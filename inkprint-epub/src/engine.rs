//! Layout analysis (PP-DocLayout-S) and OCR (PP-OCRv6 tiny) through oar-ocr
//! on ONNX Runtime. Models load lazily and stay resident until released;
//! the ORT environment itself is never torn down (its global destructors
//! abort the process on Android).

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use image::RgbImage;
use oar_ocr::core::config::OrtSessionConfig;
use oar_ocr::domain::tasks::layout_detection::LayoutDetectionConfig;
use oar_ocr::oarocr::{OAROCRBuilder, OAROCR};
use oar_ocr::predictors::LayoutDetectionPredictor;

use crate::geom::Rect;
use crate::layout::Kind;
use crate::text::Line;
use crate::Error;

pub const LAYOUT_MODEL: &str = "pp-doclayout-s.onnx";
pub const DET_MODEL: &str = "pp-ocrv6_tiny_det.onnx";
pub const REC_MODEL: &str = "pp-ocrv6_tiny_rec.onnx";
pub const REC_DICT: &str = "ppocrv6_tiny_dict.txt";

pub struct Engine {
    layout: LayoutDetectionPredictor,
    ocr: OAROCR,
}

static ORT: OnceLock<Result<(), String>> = OnceLock::new();
static ENGINE: Mutex<Option<Arc<Engine>>> = Mutex::new(None);

/// Loads libonnxruntime once. On Android `lib` is the bare
/// `libonnxruntime.so` shipped by the onnxruntime-android AAR.
fn init_ort(lib: &str) -> Result<(), Error> {
    ORT.get_or_init(|| {
        ort::init_from(lib)
            .map_err(|e| format!("cannot load ONNX Runtime from {lib}: {e}"))?
            .commit();
        Ok(())
    })
    .clone()
    .map_err(Error::Model)
}

fn session_config(threads: usize) -> OrtSessionConfig {
    // Memory pattern caching is per input shape; every page differs, so it
    // only grows memory (~960 → ~600 MB peak on device without it).
    OrtSessionConfig::new()
        .with_intra_threads(threads.max(1))
        .with_memory_pattern(false)
}

/// The resident engine, loading it on first use.
pub fn get(ort_lib: &str, models: &Path, threads: usize) -> Result<Arc<Engine>, Error> {
    let mut slot = ENGINE.lock().unwrap();
    if let Some(e) = slot.as_ref() {
        return Ok(e.clone());
    }
    init_ort(ort_lib)?;
    let path = |f: &str| {
        let p = models.join(f);
        if p.is_file() {
            Ok(p)
        } else {
            Err(Error::Model(format!("missing model file {}", p.display())))
        }
    };
    let layout = LayoutDetectionPredictor::builder()
        .with_config(LayoutDetectionConfig::with_pp_structurev3_defaults())
        .model_name("pp_doclayout_s")
        .with_ort_config(session_config(threads))
        .build(path(LAYOUT_MODEL)?.as_path())
        .map_err(|e| Error::Model(format!("layout model: {e}")))?;
    let det = path(DET_MODEL)?;
    let rec = path(REC_MODEL)?;
    let dict = path(REC_DICT)?;
    let ocr = OAROCRBuilder::new(det.as_path(), rec.as_path(), dict.as_path())
        .ort_session(session_config(threads))
        // Batched recognition pads short lines with black up to the widest
        // line in the batch, and the recognizer then silently drops
        // characters. One line at a time avoids padding altogether.
        .region_batch_size(1)
        .build()
        .map_err(|e| Error::Model(format!("OCR models: {e}")))?;
    let engine = Arc::new(Engine { layout, ocr });
    *slot = Some(engine.clone());
    tracing::info!("layout + OCR models loaded from {}", models.display());
    Ok(engine)
}

/// Drops the models (their sessions free their memory). The next
/// conversion loads them again.
pub fn release() {
    if ENGINE.lock().unwrap().take().is_some() {
        tracing::info!("layout + OCR models released");
    }
}

impl Engine {
    /// Layout regions in page pixels.
    pub fn layout(&self, page: &RgbImage) -> Result<Vec<(Rect, Kind)>, Error> {
        let out = self
            .layout
            .predict(vec![page.clone()])
            .map_err(|e| Error::Model(format!("layout: {e}")))?;
        Ok(out
            .elements
            .into_iter()
            .next()
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                let b = &e.bbox;
                (Rect::new(b.x_min(), b.y_min(), b.x_max(), b.y_max()), Kind::from_label(&e.element_type))
            })
            .collect())
    }

    /// Text lines found on the page. `scale` (pixels per point) turns the
    /// line height into an approximate font size.
    pub fn ocr(&self, page: &RgbImage, scale: f32) -> Result<Vec<Line>, Error> {
        let mut out = self
            .ocr
            .predict(vec![page.clone()])
            .map_err(|e| Error::Model(format!("OCR: {e}")))?;
        let Some(res) = out.pop() else { return Ok(Vec::new()) };
        Ok(res
            .text_regions
            .iter()
            .filter_map(|r| {
                let text = r.text.as_deref()?.trim();
                if text.is_empty() {
                    return None;
                }
                let b = &r.bounding_box;
                let rect = Rect::new(b.x_min(), b.y_min(), b.x_max(), b.y_max());
                // A text line box is roughly 1.3× the font size.
                let size = rect.h() / scale / 1.3;
                Some(Line { rect, text: text.to_string(), size })
            })
            .collect())
    }
}
