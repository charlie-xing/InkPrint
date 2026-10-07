# Layout and OCR models

Used by the EPUB printer (`inkprint-epub`), loaded through ONNX Runtime.

| File | Model | Source |
|---|---|---|
| `pp-doclayout-s.onnx` | PP-DocLayout-S | https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-doclayout-s.onnx |
| `pp-ocrv6_tiny_det.onnx` | PP-OCRv6 tiny detection | https://github.com/GreatV/oar-ocr/releases/download/v0.7.0/pp-ocrv6_tiny_det.onnx |
| `pp-ocrv6_tiny_rec.onnx` | PP-OCRv6 tiny recognition | https://github.com/GreatV/oar-ocr/releases/download/v0.7.0/pp-ocrv6_tiny_rec.onnx |
| `ppocrv6_tiny_dict.txt` | its character dictionary | https://github.com/GreatV/oar-ocr/releases/download/v0.7.0/ppocrv6_tiny_dict.txt |

The models are PaddlePaddle's PaddleOCR / PaddleX models (Apache-2.0),
exported to ONNX by the oar-ocr project. `SHA256SUMS` pins the exact files;
the app copies them to its private storage on first use.
