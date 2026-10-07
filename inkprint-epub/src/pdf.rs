//! PDF access through pdfium: text layer → positioned lines, image object
//! positions, page rendering.

use std::sync::OnceLock;

use image::RgbImage;
use pdfium_render::prelude::*;

use crate::geom::Rect;
use crate::text::Line;
use crate::Error;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

/// Binds libpdfium once per process. `lib` is a path or, on Android, the bare
/// `libpdfium.so` the app's linker namespace resolves from the APK.
pub fn pdfium(lib: &str) -> Result<&'static Pdfium, Error> {
    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }
    let bindings = Pdfium::bind_to_library(lib)
        .map_err(|e| Error::Pdf(format!("cannot load pdfium from {lib}: {e}")))?;
    Ok(PDFIUM.get_or_init(|| Pdfium::new(bindings)))
}

/// What the text layer and object list say about one page.
pub struct PageInfo {
    /// Page bounds in pixels at the analysis scale.
    pub bounds: Rect,
    pub lines: Vec<Line>,
    /// Non-whitespace characters in the text layer (visible or not).
    pub chars: usize,
    /// Placed raster images, in pixels.
    pub images: Vec<Rect>,
}

/// Reads the text layer and image placements. `scale` converts points to
/// pixels of the page render (dpi / 72).
pub fn analyze(page: &PdfPage, scale: f32) -> PageInfo {
    let page_h = page.height().value;
    let bounds = Rect::new(0.0, 0.0, page.width().value * scale, page_h * scale);
    let to_px = |r: &PdfRect| {
        Rect::new(
            r.left().value * scale,
            (page_h - r.top().value) * scale,
            r.right().value * scale,
            (page_h - r.bottom().value) * scale,
        )
    };

    let mut lines = Vec::new();
    let mut chars = 0;
    if let Ok(text) = page.text() {
        let mut cur: Option<(Line, Rect)> = None; // line, last glyph box
        let flush = |cur: &mut Option<(Line, Rect)>, lines: &mut Vec<Line>| {
            if let Some((l, _)) = cur.take() {
                if !l.text.trim().is_empty() {
                    lines.push(Line { text: l.text.trim().to_string(), ..l });
                }
            }
        };
        for c in text.chars().iter() {
            let Some(ch) = c.unicode_char() else { continue };
            // pdfium reports a hyphen at a line break as U+0002.
            let ch = if ch == '\u{2}' { '-' } else { ch };
            if ch == '\r' || ch == '\n' {
                flush(&mut cur, &mut lines);
                continue;
            }
            if ch.is_whitespace() {
                if let Some((l, _)) = cur.as_mut() {
                    if !l.text.ends_with(' ') {
                        l.text.push(' ');
                    }
                }
                continue;
            }
            if ch.is_control() {
                continue;
            }
            let Ok(b) = c.loose_bounds() else { continue };
            let r = to_px(&b);
            if r.w() <= 0.0 || r.h() <= 0.0 {
                continue;
            }
            chars += 1;
            let size = c.scaled_font_size().value;
            let new_line = match &cur {
                None => true,
                Some((line, last)) => {
                    // Compare against the whole line so short glyphs set low
                    // (CJK 、。) or high stay on it.
                    let h = line.rect.h().min(r.h());
                    let overlap = r.y1.min(line.rect.y1) - r.y0.max(line.rect.y0);
                    overlap < 0.3 * h                    // different baseline
                        || r.x0 < last.x0 - 0.5 * h     // went back left
                        || r.x0 > last.x1 + 3.0 * h     // jumped a column gutter
                }
            };
            if new_line {
                flush(&mut cur, &mut lines);
                cur = Some((Line { rect: r, text: String::new(), size }, r));
            }
            let (l, last) = cur.as_mut().unwrap();
            l.text.push(ch);
            l.rect = l.rect.union(&r);
            l.size = l.size.max(size);
            *last = r;
        }
        flush(&mut cur, &mut lines);
    }

    let mut images = Vec::new();
    for obj in page.objects().iter() {
        if obj.as_image_object().is_none() {
            continue;
        }
        if let Ok(q) = obj.bounds() {
            let r = to_px(&q.to_rect());
            let r = Rect::new(r.x0.max(0.0), r.y0.max(0.0), r.x1.min(bounds.x1), r.y1.min(bounds.y1));
            if r.w() > 1.0 && r.h() > 1.0 {
                images.push(r);
            }
        }
    }

    PageInfo { bounds, lines, chars, images }
}

/// Renders the page to an RGB bitmap at `scale` pixels per point.
pub fn render(page: &PdfPage, scale: f32) -> Result<RgbImage, Error> {
    let w = (page.width().value * scale).round().max(1.0) as i32;
    let cfg = PdfRenderConfig::new()
        .set_target_width(w)
        .render_form_data(true)
        .set_clear_color(PdfColor::WHITE);
    let bmp = page
        .render_with_config(&cfg)
        .map_err(|e| Error::Pdf(format!("render failed: {e}")))?;
    let img = bmp
        .as_image()
        .map_err(|e| Error::Pdf(format!("bitmap conversion failed: {e}")))?;
    Ok(img.to_rgb8())
}
