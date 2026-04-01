# InkPrint 项目

## 项目概述
Android 原生应用，把电子墨水阅读器（BOOX）模拟成局域网 IPP 网络打印机。
电脑通过标准打印功能把文档以 PDF 形式"打印"到设备上保存。

## 技术架构
- Rust 核心库（inkprint-core）：IPP 协议服务器 + mDNS 广播 + PDF 存储
- Kotlin Android 壳：前台服务 + UI + 通知
- 通过 UniFFI 桥接

## 开发环境
- macOS Apple Silicon
- Android NDK r29（/opt/homebrew/share/android-ndk）
- target: aarch64-linux-android
- cargo-ndk 编译
- 目标设备：BOOX（Android 12, aarch64）
- JDK: /opt/homebrew/opt/openjdk@17（Java 25 与 Kotlin 插件不兼容）

## 当前阶段
全部阶段完成，端到端打印验证通过 ✓

## 已完成
- 端到端验证：macOS 通过自定义 PPD 成功打印 PDF 到 BOOX，文件可正常打开
- macOS 客户端配置（自定义 PPD 强制 PDF 格式，见下方说明）
- IPP 2.0 兼容性修复（response 默认版本改为 2.0，添加缺失属性）
- 新增 IPP 属性：printer-info, printer-location, printer-more-info, media-default,
  media-supported, copies-default/supported, page-ranges-supported,
  print-quality-default/supported, printer-uuid, urf-supported, pdf-versions-supported,
  image/urf 格式支持
- 修复 printer-more-info URI 生成 bug（多余冒号导致 client-error-bad-request）
- Android UI：分 tab 的打印机添加指南（macOS/Windows/Linux 详细步骤）
- 阶段五：测试与调优（scripts/test_ipp.py，磁盘空间检查，512MB 大文件保护）
- 阶段四：Android 集成（APK 构建成功，UniFFI 回调桥接完成）
- 阶段三：HTTP 服务器与 mDNS 广播（hyper 1.x + mdns-sd，12 个测试全通过）
- 阶段二：IPP 操作处理（operations.rs, printer.rs — 11 个单元测试全部通过）
- 阶段一：IPP 协议解析器（types.rs, parser.rs, response.rs — 7 个单元测试全部通过）
- 阶段零：项目初始化
  - Rust workspace + inkprint-core crate（cdylib）
  - .cargo/config.toml 交叉编译配置（NDK r29，darwin-aarch64）
  - rust-toolchain.toml 固定 nightly 工具链
  - UniFFI .udl 接口 + uniffi-bindgen 生成 Kotlin 绑定
  - Android Gradle 项目（Kotlin，minSdk 26，targetSdk 34，包名 com.inkprint.app）
  - 基础 Kotlin 骨架：PrinterService, MainActivity, BootReceiver

## 关键说明
- RUSTC 必须显式指定为 rustup nightly 工具链，否则 Homebrew cargo 会用 Homebrew rustc（无 Android target）
- NDK 路径：/opt/homebrew/share/android-ndk（r29）
- `make rust-build-android` 可一键构建 .so
- .so 构建后需手动复制：`cp libinkprint_core.so libuniffi_inkprint.so`（UniFFI 按 namespace 加载）
- 端口：6310（631 在 Android 上是特权端口）
- 文件保存路径：/storage/emulated/0/Documents/InkPrint/

## macOS 添加打印机（自定义 PPD，强制 PDF）
`lpadmin -m everywhere` 在 macOS 上会失败（driverless 工具限制），需用自定义 PPD：

```bash
cat > /tmp/inkprint.ppd << 'EOF'
*PPD-Adobe: "4.3"
*FormatVersion: "4.3"
*LanguageVersion: English
*LanguageEncoding: ISOLatin1
*Manufacturer: "InkPrint"
*ModelName: "InkPrint"
*NickName: "InkPrint PDF Printer"
*PSVersion: "(3010.000) 0"
*LanguageLevel: "3"
*ColorDevice: False
*DefaultColorSpace: Gray
*cupsVersion: 2.2
*cupsFilter2: "application/vnd.cups-pdf application/pdf 0 -"
*DefaultPageSize: A4
*PageSize A4/A4: "<</PageSize[595 842]>>setpagedevice"
*PageSize Letter/Letter: "<</PageSize[612 792]>>setpagedevice"
*DefaultPaperDimension: A4
*PaperDimension A4/A4: "595 842"
*PaperDimension Letter/Letter: "612 792"
*DefaultImageableArea: A4
*ImageableArea A4/A4: "0 0 595 842"
*ImageableArea Letter/Letter: "0 0 612 792"
EOF

lpadmin -x InkPrint 2>/dev/null
lpadmin -p InkPrint -E -v ipp://<BOOX_IP>:6310/ipp/print -P /tmp/inkprint.ppd
```

## Windows 添加打印机
Settings → Bluetooth & devices → Printers & scanners → Add device
若未自动发现：The printer that I want isn't listed → Add by IP → http://<BOOX_IP>:6310/ipp/print

## Linux 添加打印机
```bash
sudo lpadmin -p InkPrint -E -v ipp://<BOOX_IP>:6310/ipp/print -m everywhere
```
