# Third-party notices

InkPrint is MIT-licensed. The EPUB printer in the Google Play / GitHub
release builds ("full" flavor) also ships the components below. The F-Droid
build contains none of them.

| Component | Used for | License | Source |
|---|---|---|---|
| PP-OCRv6 tiny (detection, recognition, dictionary) | Text recognition on scanned pages | Apache-2.0 | PaddlePaddle / PaddleOCR — https://github.com/PaddlePaddle/PaddleOCR |
| PP-DocLayout-S | Page layout analysis | Apache-2.0 | PaddlePaddle / PaddleX — https://github.com/PaddlePaddle/PaddleX |
| oar-ocr | Running the PaddleOCR models from Rust | Apache-2.0 | https://github.com/GreatV/oar-ocr |
| ONNX Runtime | Model inference | MIT | https://github.com/microsoft/onnxruntime |
| PDFium (pdfium-binaries build) | Reading and rendering PDFs | BSD-3-Clause / Apache-2.0 | https://pdfium.googlesource.com/pdfium, https://github.com/bblanchon/pdfium-binaries |
| pdfium-render | Rust bindings for PDFium | MIT / Apache-2.0 | https://github.com/ajrcarey/pdfium-render |
| zip, image | EPUB packaging, image encoding | MIT | https://crates.io/crates/zip, https://crates.io/crates/image |

The ONNX models were exported from the PaddlePaddle originals by the oar-ocr
project; `android/app/src/full/assets/models/SHA256SUMS` pins the exact files.
