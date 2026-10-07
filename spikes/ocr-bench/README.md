# ocr-bench

EPUB 输出 P0 验证用的测速工具（设计见 `design/epub-export.md`）。独立 crate，不属于主 workspace。
以后改动 OCR / 版面分析相关代码时，用它在真机上做性能回归。

## 模式

```
ocr-bench ocr <det.onnx> <rec.onnx> <dict.txt> <threads> <out_dir> <image>...
ocr-bench structure <layout.onnx> <layout_name> <det.onnx> <rec.onnx> <dict.txt> \
                    <slanet.onnx> <table_dict.txt> <threads> <out_dir> <image>...
ocr-bench rec <rec.onnx> <dict.txt> <line_image>...     # 只跑识别，用于排查丢字
ocr-bench pdf <libpdfium 目录> <dpi> <out_dir> <file.pdf>...
```

每页输出耗时和进程内存峰值（`/proc/self/status` 的 VmHWM，Mac 上为 NaN）。
第一张图先预热一次，不计时。

环境变量：

| 变量 | 作用 |
|---|---|
| `REC_BATCH=1` | 识别批大小。**必须设为 1**，否则 oar-ocr 会丢字（见 `scripts/padding-repro.py`） |
| `MEM_PATTERN=0` | 关闭 ORT memory pattern，内存峰值大幅下降 |
| `NO_OCR=1` / `NO_TABLE=1` | structure 模式下去掉对应阶段，用于拆分耗时 |
| `RAYON_NUM_THREADS` | 前后处理的线程数 |

## 构建

Android（arm64）：先从 Maven 下载 `com.microsoft.onnxruntime:onnxruntime-android:1.30.0` 的 AAR 并解压。

```
ORT_LIB_LOCATION=<aar>/jni/arm64-v8a ORT_PREFER_DYNAMIC_LINK=1 \
  RUSTC=~/.rustup/toolchains/nightly-aarch64-apple-darwin/bin/rustc \
  ANDROID_NDK_HOME=/opt/homebrew/share/android-ndk \
  cargo ndk -t arm64-v8a build --release
```

Mac：

```
PATH=/usr/bin:$PATH cargo build --release --features host
```

`PATH` 前置 `/usr/bin` 是因为 NDK 的 clang 在 PATH 里排在系统 clang 前面，会导致链接时找不到 `clang_rt.osx`。

## 上机

把 `ocr-bench`、AAR 里的 `libonnxruntime.so`、pdfium 的 `libpdfium.so`（bblanchon/pdfium-binaries）、
模型和测试图片推到 `/data/local/tmp/ocr/`，然后：

```
adb shell "nohup sh /data/local/tmp/ocr/device-bench.sh > /data/local/tmp/ocr/bench.log 2>&1 &"
```

用 nohup 在手机端运行，USB 断开也不会中断。脚本每组之间冷却 40 秒，用 `taskset` 分别测大核和小核。

模型下载地址见 oar-ocr 的 `docs/models.md`（v0.3.0 / v0.7.0 release）。

## 脚本

- `scripts/device-bench.sh`：BOOX / Pixel 上的完整测试组
- `scripts/accuracy.py`：以 PDF 文字层（`pdftotext`）为标准答案，按字符多重集合计算召回率和精确率
- `scripts/padding-repro.py`：复现识别批量填充导致丢字的问题（需要 `onnxruntime`、`numpy`、`pillow`）

## P0 结论

见 `design/epub-export.md` 第 6 节和第 10 节。
