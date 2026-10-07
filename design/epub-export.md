# InkPrint EPUB 输出：功能扩充设计

状态：已实现（v0.4，versionCode 15）· 2026-10-07 · 实现与设计的差异见第 14 节
前置：P0 可行性验证已完成（结论见第 10 节，原始数据与工具在 `spikes/ocr-bench`）

---

## 1. 目标与范围

让用户打印时直接得到一本可重排的 EPUB 电子书，而不只是 PDF。文字、标题、图片按阅读顺序混排，
扫描件也能用本机 OCR 转成文字。

**v1 做：**

- 新增一台虚拟打印机「InkPrint EPUB」，打到它的文档会转换成 EPUB
- 有文字层的 PDF：直接抽取文字（零误差，几乎不耗时）
- 无文字层的页面（扫描件、整页图片）：本机 OCR（PP-OCRv6 tiny）
- 版面分析（PP-DocLayout-S）：识别标题、正文、图片、表格、页眉页脚，并确定阅读顺序
- 图片和**表格以截图嵌入**（灰度，适合墨水屏）
- 转换在后台排队进行，通知栏显示进度，可以取消；失败时退回保存 PDF
- 全程离线，不联网，不改变隐私承诺

**v1 不做（留到以后）：**

- 表格还原成真正的 HTML 表格（需要 PP-DocLayout-M + SLANet+，APK 再增加约 27 MB，每页慢约 1 秒）
- 公式识别（v1 公式区域作为图片）
- 竖排文字、手写体
- 对已有 PDF 文件在 App 内手动「转成 EPUB」（架构上预留，入口 v1.1 再加）
- F-Droid 版的 EPUB 功能（见 9.3）

## 2. 用户体验

### 2.1 怎么选择格式：两台打印机

服务启动后通过 mDNS 广播两台打印机，共用 6310 端口，用路径区分：

| 打印机名 | 路径 | 输出 | UUID |
|---|---|---|---|
| InkPrint | `/ipp/print` | PDF（现有行为，不变） | `a7d4b3e2-…-2f6c8d3a1b4e`（现有） |
| InkPrint EPUB | `/ipp/epub` | EPUB | 新的固定 UUID |

用户在电脑的打印对话框里选哪台，就得到哪种格式，不需要碰墨水屏。两台的 UUID 必须不同，
否则 macOS/CUPS 会把它们当成同一台打印机。

**相对 P0 方案的调整：** 原方案里还有「App 内默认输出格式」开关。仔细看过代码后，我改成了下面的
「EPUB 打印机」开关：手动按 IP 添加打印机的用户，直接在 URL 里写 `/ipp/epub` 就能选 EPUB；
如果让 `/ipp/print` 也能输出 EPUB，同一个地址在不同设置下行为不同，排查问题时会很混乱。

### 2.2 App 内新增设置（「输出」卡片）

| 设置 | 默认 | 说明 |
|---|---|---|
| 启用 EPUB 打印机 | 开 | 关闭后只广播一台 PDF 打印机，`/ipp/epub` 也拒绝请求 |
| EPUB 任务同时保留 PDF | 关 | 打开后，PDF 原件和 EPUB 一起放进保存文件夹 |
| 扫描页识别文字（OCR） | 开 | 关闭后，无文字层的页面整页以图片形式放进书里（快，但不能重排） |

偏好存在现有的 `inkprint` SharedPreferences 里。

### 2.3 转换过程中用户看到什么

1. 电脑端：打印立即显示「完成」，因为 IPP 任务在收到 PDF 时就结束了，转换是之后的事
2. BOOX 通知栏：「正在转换《XXX》 第 3/20 页」，带取消按钮
3. App 文件列表：转换中的任务显示进度条，完成后变成 `.epub` 文件
4. 完成通知：「《XXX》已转换为 EPUB」，点击用系统阅读器打开
5. 失败通知：「转换失败，已保存 PDF」，PDF 照常放进保存文件夹

### 2.4 添加打印机说明页

各平台的步骤里同时列出两台打印机；手动添加的 URL 示例给出 `ipp://<IP>:6310/ipp/epub`。

## 3. 总体架构

```
            ┌──────────────────────── inkprint-core（Rust，现有） ─────────────────────────┐
 IPP 客户端 → │ http.rs：按路径选打印机配置 → operations.rs：校验 PDF、写入 jobDir           │
            │   → 回调 on_job_received(job_id, path, name, size, output=Pdf|Epub)            │
            └────────────────────────────────────────────────────────────────────────────┘
                                         │
                       ┌─────────────────┴──────────────────┐
                 output=Pdf                           output=Epub
                       │                                    │
          JobStorage.deliver()（现有）          ConversionQueue（Kotlin，新增，单线程）
                                                            │ 调用 UniFFI
            ┌──────────────── inkprint-epub（Rust，新 crate） ──────────────────┐
            │ convert_pdf_to_epub(pdf, out, options, progress)                  │
            │   pdfium：分类页面、抽文字、抽图片、渲染                             │
            │   oar-ocr + ONNX Runtime：版面分析 S、PP-OCRv6 tiny                 │
            │   排版重建 → EPUB 3 打包                                            │
            └──────────────────────────────────────────────────────────────────┘
                                                            │
                                          JobStorage.deliver(epub[, pdf])
```

### 3.1 新建 `inkprint-epub` crate

转换代码放进单独的 workspace 成员，不放进 `inkprint-core`：

- IPP 核心保持轻量，现有 12 个测试和编译速度不受影响
- 转换逻辑可以在 Mac 上用 `cargo test` 直接跑，不需要 Android
- 用 Cargo feature 控制 OCR：`default = ["ocr"]`；关掉 `ocr` 时只依赖 pdfium（以后 F-Droid 版可能用得上）

最终仍然编译进同一个 `libinkprint_core.so`：`inkprint-core` 依赖 `inkprint-epub`，并在同一个 UDL 里
导出转换接口。这样 Kotlin 端只加载一个库，UniFFI 绑定也只有一份。

### 3.2 UniFFI 接口变化

```
namespace inkprint {
    // 现有，增加 epub_enabled
    boolean start_server(u16 port, string storage_path, string printer_name,
                         boolean epub_enabled, PrintJobListener? listener);
    ...
    // 新增
    ConversionResult convert_pdf_to_epub(string pdf_path, string epub_path,
                                         ConversionOptions options,
                                         ConversionProgress? progress);
    void cancel_conversion();
    void release_models();   // 空闲时释放模型内存
};

dictionary ConversionOptions {
    string title;
    string models_dir;       // 模型文件所在目录
    string pdfium_path;      // libpdfium.so 的完整路径
    boolean ocr;             // 对应「扫描页识别文字」开关
    u32 threads;             // ORT 线程数，默认 4
};

dictionary ConversionResult {
    boolean ok;
    string? error;
    u32 pages;
    u32 ocr_pages;           // 走了 OCR 的页数
    u32 image_pages;         // 整页图片兜底的页数
};

[Trait, WithForeign]
interface ConversionProgress {
    void on_page(u32 done, u32 total);
};

enum OutputFormat { "Pdf", "Epub" };

[Trait, WithForeign]
interface PrintJobListener {
    void on_job_received(u32 job_id, string file_path, string file_name,
                         u64 size_bytes, OutputFormat output);
};
```

`cancel_conversion` 通过一个原子标志实现，每处理完一页检查一次。

## 4. IPP 层改动（inkprint-core）

1. **打印机配置表**：`PrinterState` 增加 `profiles: [PrinterProfile; 2]`，每项包含名称、
   资源路径、UUID、`OutputFormat`。`epub_enabled = false` 时只有一项。
2. **http.rs 路由**：现在只接受 `/ipp/print` 和 `/`；改为按路径查表，`/` 仍映射到 PDF 打印机，
   未知路径和被禁用的 `/ipp/epub` 返回 404。
3. **`printer_uri_for`**：现在把 `/ipp/print` 写死在 URI 里，改为按命中的配置生成，
   保证 `printer-uri-supported`、`job-uri` 指向正确的路径。
4. **Get-Printer-Attributes**：`printer-name`、`printer-uuid`、`printer-info`、
   `printer-make-and-model` 按配置返回。
5. **JobInfo** 增加 `output: OutputFormat`；Print-Job 把它传给回调。
6. 测试：两条路径各自的属性、被禁用时返回 404、`job-uri` 路径正确。

Kotlin 端 `registerMdns` 改为注册两个 `NsdServiceInfo`（名称、`rp`、`UUID`、`ty` 不同），
各自持有一个 `RegistrationListener`。

## 5. 转换流水线（inkprint-epub）

每页处理顺序：**分类 → 版面分析 → 取文字 → 重建 → 写入**。一次只处理一页，内存不随页数增长。

### 5.1 页面分类（pdfium）

| 页面类型 | 判定 | 文字来源 |
|---|---|---|
| 文字页 | 有可见文字层，字符数 ≥ 阈值 | pdfium 文字层（含每个字符的坐标、字号、字体） |
| 带隐藏文字层的扫描页 | 文字渲染模式为不可见（已被其他软件 OCR 过） | 同上，直接用现成的文字层 |
| 图片页 | 文字很少，且有图片对象覆盖页面大部分面积 | OCR；OCR 关闭时整页作为图片 |

### 5.2 版面分析（所有页面统一走）

文字页和图片页都先按 200 DPI 渲染（BOOX 上 0.09 秒/页），再跑 PP-DocLayout-S（0.47 秒/页），
得到区块框和类别。两类页面因此共用一套「区块 → 阅读顺序 → 结构」的逻辑，区别只在每个区块的文字从哪里来：

- 文字页：把文字层的字符按坐标分到各个区块
- 图片页：对整页跑一次 OCR（检测 + 识别），把识别出的文字行按重叠面积分到区块

| 版面类别 | EPUB 中的处理 |
|---|---|
| doc_title / paragraph_title | `<h1>`–`<h3>`，级别按字号和类别推断 |
| text / abstract / content | `<p>` |
| image / chart / figure | 裁剪成灰度 JPEG，`<figure><img>` |
| table | **裁剪成图片**（v1 决定）；文字页的表格文字存进 `alt` 以便检索 |
| figure_title / table_title | `<figcaption>` |
| formula | 裁剪成图片 |
| header / footer / number | 丢弃 |
| 未识别区域 | 按正文处理 |

**兜底：** 版面模型加载失败或某页出错时，这一页退化成整页图片，不让整本书失败。

### 5.3 阅读顺序

先用版面模型给出的区块，再对区块做 XY-Cut（递归地按水平/垂直空白切分），处理双栏和图文混排。
区块内部的文字行按从上到下、从左到右排序。

### 5.4 文本重建

- **段落合并**：行尾没有句末标点、下一行缩进一致时，合并成同一段。
  中文、日文直接拼接，不加空格；英文用空格拼接，并去掉行尾连字符（`exam-\nple` → `example`）。
- **跨页段落**：上一页最后一段没有句末标点、下一页第一个区块也是正文时，合并成一段。
- **页眉页脚**：版面模型会标出；另外对「多页重复出现在页面顶部或底部的同一段文字」和页码做二次过滤。
- **标题层级**：统计全书正文字号，标题按字号排序后映射成 h1–h3。OCR 页没有字号，按文字行高度估算。
- **列表**：以 `•`、`-`、`1.`、`(1)` 开头的连续行转成列表。

### 5.5 EPUB 打包

自己用 `zip` crate 实现一个小的 EPUB 3 写入器（几百行，完全可控，便于针对墨水屏调整）：

- `mimetype` 作为第一个条目、不压缩；`META-INF/container.xml`；`OEBPS/content.opf`
- `nav.xhtml`，同时生成 `toc.ncx` 兼容老阅读器（BOOX 自带的 NeoReader 和第三方阅读器都能用）
- 分章：遇到 h1 切一章；全书没有标题时，每 20 页切一章，避免单个 XHTML 太大导致阅读器卡顿
- 封面：第一页渲染缩略图
- 元数据：标题取 IPP `job-name`；语言按 CJK 字符比例判断 zh/en；标识符用随机 UUID
- 图片：灰度、最长边 1600 px、JPEG 质量 80
- CSS 尽量少：不指定字体和字号（交给阅读器），只设段落间距、图片 `max-width:100%`、
  以及中文段落首行缩进 2em
- 每次生成后，单元测试用 `epubcheck` 校验（只在开发机上运行，不进 App）

## 6. 模型与运行时

| 项目 | 选择 | 理由（P0 数据） |
|---|---|---|
| OCR | PP-OCRv6 tiny（检测 1.7 MB + 识别 4.3 MB） | BOOX 8 核 2.2 秒/页，召回率 99.9%，内存 193 MB |
| 版面分析 | PP-DocLayout-S（4.7 MB） | BOOX 0.47 秒/页，内存 108 MB |
| 推理引擎 | ONNX Runtime 1.30（官方 `onnxruntime-android` AAR） | 用 Maven 依赖引入，Rust 端动态链接 `libonnxruntime.so` |
| Rust 封装 | oar-ocr 0.10（锁定版本） | 已在两台设备上验证 |
| PDF | pdfium（bblanchon/pdfium-binaries，`chromium/8086`） | BOOX 抽文字 4 ms/页、渲染 90 ms/页 |

**必须遵守的配置（P0 踩坑结论）：**

1. `region_batch_size(1)`：oar-ocr 批量识别时会用黑色像素填充短行，导致 SVTR 静默丢字
   （「网」「用」「面」……）。同时给上游提 issue。
2. `with_memory_pattern(false)`：每页尺寸不同，打开时内存从约 600 MB 涨到约 960 MB，速度没有好处。
3. **不使用 oar-ocr 的 `OARStructureBuilder`**：它按 PP-StructureV3 的服务器端参数先放大页面，
   OCR 部分每页约 5 秒、内存 574 MB。改为分别调用版面预测器和 OCR 预测器，自己组装。
4. **模型常驻，不在进程内反复创建和销毁 ORT 环境**：进程退出时 ORT 全局析构会触发
   `FORTIFY: pthread_mutex_lock called on a destroyed mutex`。只释放 session（`release_models`），
   不销毁全局环境。

**加载方式：**

- 模型放在 APK 的 `assets/models/`，首次转换时复制到 `filesDir/models/`（共约 11 MB，只复制一次，
  之后用文件路径加载）
- 识别和版面模型在第一次转换时懒加载，转换队列空闲 5 分钟后调用 `release_models()` 释放
- ORT 线程数：默认 4；BOOX 上 8 线程只快 15%（2.5 → 2.2 秒），4 线程能留出大核给阅读器和系统

**内存预算（BOOX 2.8 GB 机型，可用约 1.2 GB）：** pdfium + 版面 S + OCR + 当前页的位图，
目标峰值 **≤ 400 MB**，在 M3 验收时用 `ocr-bench` 的方法在 BOOX 上实测。

## 7. Android 端改动

### 7.1 转换队列 `ConversionQueue`（新增）

- 运行在 `PrinterService` 里的单线程 executor 上，一次只转一本，避免内存叠加
- 收到 EPUB 任务：PDF 留在 `jobDir` 作为暂存，同时写一个标记文件 `<name>.epub-pending`，然后入队
- 转换输出到 `cacheDir/convert/<job>.epub`，完成后调用 `JobStorage.deliver()`
  （现有逻辑：有保存文件夹就移进去，否则发布到 `Documents/InkPrint/`）
- 「同时保留 PDF」开启时 PDF 也交付；否则删除暂存的 PDF
- 失败：交付 PDF，通知用户
- **断点恢复**：服务启动时扫描 `jobDir` 里的 `*.epub-pending`，重新入队
  （进程被系统杀掉、手机重启后，任务不会丢）
- 取消：调用 `cancel_conversion()`，然后交付 PDF

### 7.2 其他改动

| 文件 | 改动 |
|---|---|
| `PrinterService.kt` | 注册两个 mDNS 服务；`onJobReceived` 按格式分流；转换进度通知（`setProgress`） |
| `JobStorage.kt` | `mimeTypeOf` 增加 `epub → application/epub+zip`；`deliver` 支持一次交付多个文件 |
| `MainActivity.kt` | 「输出」设置卡片；文件列表显示转换中的任务和进度；添加打印机说明里加入 EPUB 打印机 |
| `FolderBrowser.kt` | `.epub` 图标 |
| `InkPrintLib.kt` | 加载顺序：`onnxruntime` → `inkprint_core`（pdfium 按路径动态加载） |
| `build.gradle.kts` | 增加 `com.microsoft.onnxruntime:onnxruntime-android:1.30.0`；模型放进 assets；`versionCode` 15 |

### 7.3 构建

- `Makefile` 新增 `fetch-native-deps`：下载 pdfium 的 Android 版到 `jniLibs/`（已被 gitignore），
  **校验 SHA-256**；从 Gradle 缓存的 ORT AAR 里取出 `libonnxruntime.so`，供 Rust 编译时链接
  （`ORT_LIB_LOCATION` + `ORT_PREFER_DYNAMIC_LINK=1`）
- 模型文件**提交进仓库**（约 11 MB，Apache-2.0），放在 `android/app/src/main/assets/models/`，
  附 `SHA256SUMS` 和来源说明；不在构建时下载，保证构建可复现

### 7.4 APK 体积

| 组成 | 压缩后 |
|---|---|
| ONNX Runtime | 11.8 MB |
| pdfium | 3.1 MB |
| PP-OCRv6 tiny + 字典 | 5.4 MB |
| PP-DocLayout-S | 4.2 MB |
| oar-ocr 等 Rust 代码增量 | 约 3–5 MB（待实测） |
| **合计** | **约 28–30 MB** |

以后可以用 ONNX Runtime 的精简编译（只保留这两个模型用到的算子）把 11.8 MB 压到几 MB，不在 v1 范围内。

## 8. 错误处理

| 情况 | 处理 |
|---|---|
| PDF 加密或损坏，pdfium 打不开 | 转换失败，交付 PDF |
| 某页版面分析或 OCR 出错 | 这一页整页作为图片，其他页照常 |
| 模型文件缺失或损坏 | 关闭 OCR，所有图片页整页作为图片；在日志中记录 |
| 存储空间不足（低于 PDF 大小 × 3 + 50 MB） | 不开始转换，交付 PDF |
| 转换中进程被杀 | 下次启动时根据 `.epub-pending` 标记重新转换 |
| 用户取消 | 交付 PDF |
| 超大文档（> 500 页） | 照常转换；通知里显示预计剩余时间 |

## 9. 隐私、许可与发布

### 9.1 隐私

数据流不变：文档不离开设备，没有网络请求，模型随 APK 打包、不在线下载。
隐私政策（中英文）在「设备上处理的信息」里补一行：「选择 EPUB 打印机时，在本机进行版面分析和文字识别，
生成的 EPUB 与 PDF 同样只保存在设备上」。发布 v1 时同步更新 blog.xcl.name 上的两份隐私政策。

### 9.2 第三方许可

| 组件 | 许可 |
|---|---|
| PaddleOCR 模型（PP-OCRv6、PP-DocLayout） | Apache-2.0 |
| oar-ocr | Apache-2.0 |
| ONNX Runtime | MIT |
| pdfium | BSD-3-Clause / Apache-2.0 |

都和项目的 MIT 许可兼容。App 里增加「开源许可」页面，并在仓库根目录增加 `THIRD_PARTY_NOTICES.md`。

### 9.3 F-Droid

F-Droid 要求所有原生库都从源码构建。ONNX Runtime 和 pdfium 从源码构建的成本很高。v1 的方案：
增加一个 `fdroid` 构建变体，不包含 EPUB 打印机，行为和现在完全一样；Play 版和 GitHub Release 版
包含 EPUB。以后如果找到纯 Rust 的 PDF 方案，再考虑让 F-Droid 版支持「仅文字层」的 EPUB。

## 10. P0 验证数据（摘要）

BOOX Note Air（骁龙 636，2.8 GB 内存，Android 10）：

| 环节 | 每页耗时 | 内存峰值 |
|---|---|---|
| pdfium 抽取文字层（含字符坐标） | 0.004 秒 | 95 MB |
| pdfium 渲染 200 DPI | 0.09 秒 | 同上 |
| 版面分析 PP-DocLayout-S（4 大核） | 0.47 秒 | 108 MB |
| PP-OCRv6 tiny（8 核 / 4 大核 / 4 小核） | 2.2 / 2.5 / 4.0 秒 | 193 MB |

由此估算 v1 在 BOOX 上：

- **文字页**：渲染 + 版面分析 + 重建，约 0.6 秒/页，100 页约 1 分钟
- **扫描页**：再加 OCR，约 3 秒/页，100 页约 5 分钟
- OCR 准确率（字符召回率）：PP-OCRv6 tiny 99.9%；测试集是隐私政策页面和模拟扫描件，
  M3 阶段要补充真实扫描件、双栏论文、图文混排杂志等样本

## 11. 测试计划

| 层次 | 内容 |
|---|---|
| Rust 单元测试（inkprint-core） | 两条路径的属性、`job-uri`、禁用 EPUB 时 404、回调里的格式 |
| Rust 单元测试（inkprint-epub） | 段落合并（中/英/混合）、去连字符、页眉页脚过滤、XY-Cut、分章、EPUB 结构 |
| 样例回归 | `inkprint-epub/tests/fixtures/` 放几份小 PDF（由仓库内 HTML 生成，无版权问题），对比生成的 XHTML 快照；`epubcheck` 校验 |
| 性能回归 | `spikes/ocr-bench` 在 BOOX 和 Pixel 上测每页耗时和内存峰值，M3 验收：BOOX 扫描页 ≤ 3.5 秒、峰值 ≤ 400 MB |
| 端到端 | macOS `lpadmin` 分别添加两台打印机并打印（沿用现有验证方法）；检查 Windows、iOS 的发现结果；用 NeoReader 打开生成的 EPUB |
| 真机 | BOOX Note Air（最低配置）+ Pixel 10a |

## 12. 里程碑与提交顺序

按「先文字层、后 OCR」的顺序提交，每个里程碑结束时都是一个可以发布的状态。

| 里程碑 | 内容 | 可发布状态 |
|---|---|---|
| **M0** | 提交 `spikes/ocr-bench` 和本设计文档 | — |
| **M1** | IPP 双打印机、格式路由、mDNS 双注册、Kotlin 转换队列骨架、设置项 | EPUB 打印机可用，但暂时只把 PDF 每页渲染成图片打包成 EPUB |
| **M2** | `inkprint-epub`：pdfium 文字层 → 段落 / 标题 / 图片重建 → EPUB 写入器（暂不用版面模型，靠字号和坐标的启发式规则） | **有文字层的文档能生成真正可重排的 EPUB**；扫描页仍是整页图片 |
| **M3** | 引入 ONNX Runtime + 版面分析 S + PP-OCRv6 tiny；扫描页 OCR；表格和图片区域截图；页眉页脚识别改用模型结果 | 完整功能 |
| **M4** | 进度通知和取消、断点恢复、开源许可页、隐私政策更新、F-Droid 变体、versionCode 15 | 正式发布 |

M2 先用启发式规则、M3 再加版面模型，是为了让 M2 不依赖 ONNX Runtime，可以先发布和收集反馈。
M3 的版面模型接入后，文字页和图片页统一走 5.2 节的流程。

## 13. 待定问题

1. **EPUB 打印机的名字**：「InkPrint EPUB」还是「InkPrint (EPUB)」？Windows 和 macOS 显示打印机名时括号会不会出问题，M1 时实测。
2. **分章阈值**：没有标题时每 20 页一章是拍脑袋的数字，M2 时用 NeoReader 实测大文件的翻页性能后再定。
3. **OCR 线程数**：默认 4，要不要在设置里开放？倾向于不开放，避免用户困惑。
4. **iOS**：iOS 会不会把两台打印机都显示出来、选 EPUB 打印机是否正常，M1 时需要一台 iPhone/iPad 测试。

## 14. 实现记录（2026-10-07）

M0–M4 一次完成。提交顺序：`inkprint-epub`（独立库）→ core（双打印机 + 转换接口）→ Android（队列、界面、构建变体）。
文字层路线和 OCR 路线在同一个库里，用 `ocr` feature 区分，没有拆成两次发布。

### 与设计的差异

| 设计 | 实际 | 原因 |
|---|---|---|
| Rust 编译时动态链接 `libonnxruntime.so`（`ORT_LIB_LOCATION`） | `ort` 的 `load-dynamic`：运行时 dlopen | 构建不需要 ORT 库文件；F-Droid 变体和 Mac 测试都更简单 |
| `jobDir` 里放 `.epub-pending` 标记文件 | 待转换的 PDF 移到 `convert/` 目录，目录里的 PDF 就是待办任务 | 选择保存文件夹时 `adoptStagedJobs` 会搬走 `jobDir` 里的文件，转换中的 PDF 不能放在那里 |
| 表格只依赖版面模型 | 另加基于文字列对齐的表格检测（≥3 行、≥2 列、列起点对齐，排除全宽双栏正文） | PP-DocLayout-S 在测试文档里漏掉了大部分表格 |
| 标题只依赖版面模型 | 模型之外仍按字号判定（≥ 正文 1.12 倍、≤ 60 字） | 模型把不少小标题判成了正文 |
| — | 模型结果的修正：包住其他区块的「容器」文字框丢弃；页边距之外的「页眉/页脚」改判为正文；超过 80 字或 3 行的「标题」改判为正文；中心点落在图片/表格内的文字行归入该区域 | 实测中这几类错误都会打乱阅读顺序或丢内容 |
| 模型从 assets 复制，每次升级重复复制 | 按 `versionCode` 写 `.version` 标记，版本不变不重复复制 | 省掉每次启动的 11 MB 拷贝 |

### BOOX Note Air 实测（release 包，通过 `/ipp/epub` 打印）

| 文档 | 页数 | 用时 | 说明 |
|---|---|---|---|
| 中文隐私政策 | 4 | 3.3 秒 | 含首次加载模型 |
| 英文隐私政策 | 5 | 3.3 秒 | |
| 扫描件 | 2 | 6.6 秒 | 两页都走 OCR |
| 长文档 | 45 | 27.7 秒 | 约 0.6 秒/页 |

模型常驻时原生堆约 180 MB。四份结果都通过 epubcheck 5.4.0（0 错误、0 警告），NeoReader 打开正常。

### 体积

full 变体 release APK 64.7 MB（`.so` 不压缩存放），其中 ONNX Runtime 31.5 MB、核心库 6.5 MB（release 配置 `strip = "symbols"`，原来 11.7 MB）、
pdfium 6.3 MB、模型 11 MB。fdroid 变体 9.1 MB。

### 仍待验证

- iOS / iPadOS 是否同时显示两台打印机（第 13 节问题 4）
- Windows 添加「InkPrint EPUB」（第 13 节问题 1）
- 大文件在 NeoReader 中的翻页性能，以确定无标题时的分章页数（第 13 节问题 2，目前 20 页）
- 真实扫描件、双栏论文、图文杂志等更多样本
