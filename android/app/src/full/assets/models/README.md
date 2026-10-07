# Layout and OCR models

Used by the EPUB printer (`inkprint-epub`), loaded through ONNX Runtime.

| File | Model | Source |
|---|---|---|
| `pp-doclayout-s.onnx` | PP-DocLayout-S | https://github.com/GreatV/oar-ocr/releases/download/v0.3.0/pp-doclayout-s.onnx |
| `pp-ocrv6_tiny_det.onnx` | PP-OCRv6 tiny detection | https://github.com/GreatV/oar-ocr/releases/download/v0.7.0/pp-ocrv6_tiny_det.onnx |
| `pp-ocrv6_small_rec.onnx` | PP-OCRv6 small recognition | https://github.com/GreatV/oar-ocr/releases/download/v0.7.0/pp-ocrv6_small_rec.onnx |
| `ppocrv6_dict.txt` | its character dictionary (18,708 characters, simplified and traditional) | https://github.com/GreatV/oar-ocr/releases/download/v0.7.0/ppocrv6_dict.txt |

Recognition uses the small model rather than tiny: tiny's 6,904-character
dictionary misses common traditional characters (衝, 說, 嗎, 讓, ...).

The models are PaddlePaddle's PaddleOCR / PaddleX models (Apache-2.0),
exported to ONNX by the oar-ocr project. `SHA256SUMS` pins the exact files;
the app copies them to its private storage on first use.
