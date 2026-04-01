# InkPrint 项目 — Claude Code 交互提示词

> 以下内容分阶段使用，每个阶段作为一次独立的 Claude Code session 提示。

---

## 阶段零：项目初始化

```
我要创建一个 Android 项目 "InkPrint"（墨印），功能是把 Android 电子墨水阅读器（BOOX）模拟成一台局域网 IPP 网络打印机，局域网内的电脑可以通过标准打印功能把文档以 PDF 形式"打印"到这台设备上保存。

项目架构：
- Rust 核心库（inkprint-core）：实现 IPP 协议服务器、mDNS 服务发现广播、PDF 文件存储
- Kotlin Android 壳：前台服务、UI界面、通知、文件管理
- 通过 UniFFI 桥接 Rust 与 Kotlin

请帮我初始化项目结构：
1. 创建 Rust workspace，包含 inkprint-core crate
2. 配置 Cargo.toml，目标为 cdylib，依赖：tokio (full), hyper 1.x (server, http1), mdns-sd, uniffi, bytes, chrono, tracing, tracing-android
3. 配置 .cargo/config.toml 用于 aarch64-linux-android 交叉编译（NDK linker）
4. 创建 Android Gradle 项目（Kotlin，minSdk 26，targetSdk 34），包名 com.inkprint.app
5. 配置 cargo-ndk 集成到 Gradle build 流程
6. 创建基础 UniFFI .udl 接口文件，先定义一个简单的 start_server(port: u16, storage_path: string) -> bool 和 stop_server() -> bool

先不实现业务逻辑，只搭好骨架确保 Rust 能编译为 .so 并被 Kotlin 调用成功。给我一个最小可验证的 hello-world 级别联通测试。
```

---

## 阶段一：IPP 协议解析器

```
继续 InkPrint 项目。现在实现 IPP 协议的二进制解析与响应构建。

在 inkprint-core/src/ipp/ 目录下创建：

1. types.rs — 定义 IPP 核心数据结构：
   - IppVersion (major, minor)
   - IppOperationId 枚举（至少：PrintJob=0x0002, GetJobAttributes=0x0009, GetPrinterAttributes=0x000B）
   - IppStatusCode 枚举（至少：SuccessfulOk=0x0000, ClientBadRequest=0x0400, ServerInternalError=0x0500）
   - IppAttributeGroup（delimiter-tag + Vec<IppAttribute>）
   - IppAttribute（name: String, value: IppValue）
   - IppValue 枚举（Integer, Boolean, Enum, TextWithoutLanguage, NameWithoutLanguage, DateTime, Uri, Charset, NaturalLanguage, MimeMediaType, OctetString, NoValue）
   - IppRequest 和 IppResponse 结构体

2. parser.rs — 从 &[u8] 解析 IppRequest：
   - 解析 version, operation-id, request-id
   - 逐个解析 attribute group（按 delimiter tag 0x01-0x05 分组）
   - 每个 attribute 按 tag-value encoding 解析
   - 剩余 bytes 作为 document-data 保留
   - 详细的错误处理，返回 Result<IppRequest, IppError>

3. response.rs — 构建 IppResponse 并序列化为 bytes：
   - 构建标准响应头（version + status-code + request-id）
   - 序列化 attribute groups
   - 提供 builder 模式方便组装

IPP 二进制格式参考：
- 每个 attribute：value-tag(1B) + name-length(2B) + name + value-length(2B) + value
- group delimiter tags: 0x01=operation, 0x02=job, 0x04=printer, 0x03=end
- 整数是 4 字节大端序

写完后加上单元测试，构造一个 Get-Printer-Attributes 请求的 raw bytes，验证解析结果正确。再构造一个响应，验证序列化后可以被重新解析。
```

---

## 阶段二：IPP 操作处理

```
继续 InkPrint 项目。现在实现 IPP 操作处理逻辑。

创建 inkprint-core/src/ipp/operations.rs 和 inkprint-core/src/ipp/printer.rs：

1. printer.rs — PrinterState 结构体：
   - printer_name: String
   - printer_uri: String（运行时根据本机 IP 生成）
   - storage_dir: PathBuf
   - job_counter: AtomicU32
   - active_jobs: DashMap<u32, JobInfo>
   - JobInfo { id, state, name, originating_user, time_created, file_path, size_bytes }

2. operations.rs — 处理三个核心操作：

   handle_get_printer_attributes(request) -> IppResponse：
   - 返回打印机属性集，包括：
     printer-uri-supported, printer-name, printer-state(3=idle),
     document-format-supported(["application/pdf","application/postscript","application/octet-stream"]),
     operations-supported([0x0002, 0x0009, 0x000B]),
     charset-supported("utf-8"), natural-language-configured("en"),
     printer-is-accepting-jobs(true), pdl-override-supported("not-attempted"),
     printer-make-and-model("InkPrint Virtual PDF Printer")
   - 只返回 requested-attributes 中指定的属性（如果有的话）

   handle_print_job(request, document_data) -> IppResponse：
   - 从请求中提取 job-name, requesting-user-name, document-format
   - 生成 job-id（递增计数器）
   - 如果 document-format 是 application/pdf，直接将 document_data 写入 storage_dir
   - 文件名格式：{timestamp}_{job_name_sanitized}.pdf
   - 返回 job-id, job-uri, job-state(9=completed) 等属性
   - 通过回调通知 Android 层有新文件到达

   handle_get_job_attributes(request) -> IppResponse：
   - 根据 job-id 查询 active_jobs 返回任务信息

3. 实现一个 dispatch(request) -> response 函数根据 operation-id 路由到对应 handler

加单元测试覆盖每个操作。
```

---

## 阶段三：HTTP 服务器与 mDNS 广播

```
继续 InkPrint 项目。实现 HTTP 服务器层和 mDNS 服务发现。

1. server/http.rs — 基于 hyper 1.x 的 HTTP 服务器：
   - 监听指定端口（默认 631）
   - 只接受 POST 请求到 /ipp/print 路径
   - Content-Type 必须是 application/ipp
   - 读取完整 request body
   - 调用 IPP parser 解析
   - 调用 operations::dispatch 处理
   - 将 IppResponse 序列化后返回，Content-Type: application/ipp
   - 错误处理：非法请求返回 HTTP 400 或 IPP client-error

2. server/listener.rs — 服务生命周期管理：
   - start(config: ServerConfig) -> Result<ServerHandle>
     ServerConfig { port, storage_dir, printer_name, on_job_complete: callback }
   - ServerHandle 持有 tokio runtime 和 shutdown signal
   - stop() 优雅关闭
   - 获取本机局域网 IP 地址用于构建 printer-uri

3. mdns/advertiser.rs — 基于 mdns-sd crate：
   - 注册服务：service_type = "_ipp._tcp.local."
   - 设置 TXT 记录：
     txtvers=1, pdl=application/pdf, rp=ipp/print,
     product=(InkPrint), ty=InkPrint Virtual Printer,
     adminurl=http://{ip}:{port}/, priority=0
   - 提供 start_advertising() 和 stop_advertising()
   - macOS 和 Windows 都能通过这个自动发现打印机

4. 更新 ffi.rs / .udl：
   - 暴露 start_server(port, storage_dir, printer_name) 和 stop_server()
   - 暴露回调接口 trait PrintJobCallback { fn on_job_received(job_id, file_path, file_name, size_bytes) }

写一个集成测试：启动服务器，用 reqwest 构造一个 Print-Job IPP 请求（带一个小 PDF），验证文件被保存到目标目录。
```

---

## 阶段四：Android 集成

```
继续 InkPrint 项目。实现 Android 端集成。

1. PrinterService.kt — 前台服务：
   - 调用 Rust FFI 的 start_server()，传入端口和存储路径
   - 存储路径默认为 Environment.getExternalStoragePublicDirectory(Documents)/InkPrint/
   - 前台通知显示服务状态（运行中/已停止）和本机 IP 地址
   - 实现 PrintJobCallback 接口，当 Rust 层回调 on_job_received 时：
     a. 发送系统通知 "收到新文档: {filename} ({size})"
     b. 通知点击后用 Intent 打开 PDF 文件
     c. 发送广播通知 UI 更新
   - 在 onDestroy 中调用 stop_server()

2. MainActivity.kt — 主界面（Jetpack Compose）：
   - 顶部大按钮：启动/停止打印服务（绿色/红色状态指示）
   - 显示当前 IP 地址和端口
   - 显示简要使用说明（如何在 macOS/Windows 添加打印机）
   - 底部：最近收到的文件列表（最近 10 条）
   - 点击文件条目打开 PDF

3. BootReceiver.kt — 开机自启：
   - 监听 BOOT_COMPLETED 广播
   - 根据 SharedPreferences 中的设置决定是否自动启动服务

4. AndroidManifest.xml 权限：
   - INTERNET, ACCESS_NETWORK_STATE, ACCESS_WIFI_STATE
   - FOREGROUND_SERVICE, FOREGROUND_SERVICE_SPECIAL_USE
   - RECEIVE_BOOT_COMPLETED
   - READ/WRITE_EXTERNAL_STORAGE（或 MANAGE_EXTERNAL_STORAGE for Android 11+）
   - POST_NOTIFICATIONS (Android 13+)

确保 Gradle 构建流程：先 cargo-ndk 编译 Rust -> uniffi-bindgen 生成 .kt -> 编译 APK。
给我完整可编译的代码和一步步的构建命令。
```

---

## 阶段五：测试与调优

```
继续 InkPrint 项目。帮我测试和调优。

1. 编写端到端测试脚本（Python 或 shell）：
   - 用 ipptool（CUPS 自带工具）向 InkPrint 发送标准 IPP 请求验证合规性
   - 命令示例：ipptool -tv ipp://<device-ip>:631/ipp/print get-printer-attributes.test
   - 发送实际 Print-Job 验证文件保存

2. 兼容性检查清单和修复：
   - macOS: 系统偏好设置 → 打印机 → 添加 → 应能自动发现 InkPrint
   - Windows 10/11: 设置 → 打印机 → 添加打印机 → 应能发现（可能需要额外的 TXT 记录）
   - Linux (CUPS): lpadmin 添加 IPP 打印机
   - 如果某平台发现有问题，告诉我需要调整什么

3. 性能考量：
   - 大文件处理：Print-Job 可能收到几十MB的 PDF，确保不会 OOM（流式写入磁盘而非全部加载到内存）
   - 并发处理：支持同时接收多个打印任务
   - 电子墨水屏省电：服务空闲时 CPU 占用应趋近于零

4. 错误恢复：
   - 网络断开重连后 mDNS 重新广播
   - 存储空间不足时拒绝任务并返回合适的 IPP 错误码
   - WiFi 切换时自动更新 printer-uri 和重新广播
```

---

## 使用技巧

和 Claude Code 交互时的注意事项：

1. **每个阶段单独一个 session**，阶段完成后验证可编译/可运行再进入下一阶段
2. **提前告诉 Claude Code 你的环境**，在每个 session 开头补充：
   ```
   我的开发环境：
   - macOS (Apple Silicon)
   - Android NDK r26c，已配置 ANDROID_NDK_HOME
   - rustup target add aarch64-linux-android 已完成
   - cargo-ndk 已安装
   - Android Studio Hedgehog+
   - 目标设备：BOOX Tab Ultra (Android 12, aarch64)
   ```
3. **遇到编译错误直接贴**，Claude Code 擅长修复交叉编译问题
4. **阶段三完成后可以先在桌面 Linux/macOS 上测试** Rust 库（不需要 Android），确认 IPP 协议正确再上设备
