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
    /// Lines were dropped for a font with a broken Unicode map: their text
    /// is only in the pixels.
    pub garbled: bool,
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
    let mut garbled = false;
    if let Ok(text) = page.text() {
        // A font with a broken ToUnicode map reads as letters from scripts
        // no page mixes. Lines set (even partly) in such a font are dropped:
        // the page then has text the layer doesn't explain, which OCR reads.
        let bad_fonts = garbled_fonts(text.chars().iter().filter_map(|c| {
            let ch = c.unicode_char()?;
            (!ch.is_ascii() && !ch.is_whitespace()).then(|| (c.font_name(), ch))
        }));
        if !bad_fonts.is_empty() {
            tracing::info!("text layer: fonts with a broken Unicode map, read by OCR instead: {bad_fonts:?}");
        }
        // line, last glyph box, set in a broken font
        let mut cur: Option<(Line, Rect, bool)> = None;
        let mut dropped = 0;
        let mut flush = |cur: &mut Option<(Line, Rect, bool)>, lines: &mut Vec<Line>| {
            if let Some((l, _, bad)) = cur.take() {
                if bad {
                    dropped += l.text.chars().filter(|c| !c.is_whitespace()).count();
                } else if !l.text.trim().is_empty() {
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
                if let Some((l, _, _)) = cur.as_mut() {
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
                Some((line, last, _)) => {
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
                cur = Some((Line { rect: r, text: String::new(), size, chars: Vec::new(), ocr: false }, r, false));
            }
            let (l, last, bad) = cur.as_mut().unwrap();
            l.text.push(ch);
            l.rect = l.rect.union(&r);
            l.size = l.size.max(size);
            *last = r;
            *bad |= !ch.is_ascii() && !bad_fonts.is_empty() && bad_fonts.contains(&c.font_name());
        }
        flush(&mut cur, &mut lines);
        chars -= dropped;
        garbled = dropped > 0;
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

    PageInfo { bounds, lines, chars, images, garbled }
}

/// Fonts whose text looks like raw glyph codes read through a wrong
/// Unicode map rather than writing: more than a tenth of their non-ASCII
/// letters fall in blocks real text all but never uses — CJK Extension A,
/// Yi, radicals, Syriac to NKo, Cherokee, Tibetan, private use and the
/// like. A wrong map scatters codes over the whole of Unicode, so they turn
/// up quickly. Takes `(font, char)` for the page's non-ASCII characters.
pub fn garbled_fonts(chars: impl Iterator<Item = (String, char)>) -> Vec<String> {
    let mut fonts: Vec<(String, usize, usize)> = Vec::new(); // font, letters, odd
    for (font, c) in chars {
        let i = match fonts.iter().position(|f| f.0 == font) {
            Some(i) => i,
            None => {
                fonts.push((font, 0, 0));
                fonts.len() - 1
            }
        };
        fonts[i].1 += 1;
        if is_odd(c) {
            fonts[i].2 += 1;
        }
    }
    fonts.into_iter().filter(|&(_, n, odd)| odd * 10 > n).map(|f| f.0).collect()
}

fn is_odd(c: char) -> bool {
    matches!(c as u32,
        0x0250..=0x02AF   // IPA
        | 0x0530..=0x058F // Armenian
        | 0x0700..=0x08FF // Syriac, Thaana, NKo, Samaritan, Mandaic, …
        | 0x0F00..=0x0FFF // Tibetan
        | 0x1200..=0x1DFF // Ethiopic, Cherokee, Canadian syllabics, Runic, …
        | 0x2900..=0x2BFF // supplemental arrows and maths
        | 0x2C00..=0x2FDF // Glagolitic … CJK radicals, Kangxi
        | 0x3200..=0x33FF // enclosed CJK, CJK compatibility
        | 0x3400..=0x4DBF // CJK Extension A
        | 0xA000..=0xA4CF // Yi
        | 0xE000..=0xF8FF // private use
    )
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

#[cfg(test)]
mod tests {
    use super::garbled_fonts;

    fn fonts(pages: &[(&str, &str)]) -> Vec<String> {
        garbled_fonts(pages.iter().flat_map(|(f, t)| {
            t.chars().filter(|c| !c.is_ascii() && !c.is_whitespace()).map(|c| (f.to_string(), c))
        }))
    }

    #[test]
    fn fonts_with_wrong_unicode_maps_are_found() {
        // Text layers of a magazine PDF whose ToUnicode maps are broken,
        // next to fonts of the same page that read fine.
        let page = [
            ("Body", "每當攝影師加布里爾·卡林伯迪在旅途中遇到當地人，他都會提出同樣的問題：我可以看看你的藥櫃裡有什麼嗎？"),
            ("Caption", "䕧⫹佦✲ 䱳程㉬玑 㖈齡牟翱涸嵳抂♳侸⼧䎃⢵♧そ伢䕧䌌♶倬ꅾ鵦開剋痧嵳抂䬝伢"),
            ("Big5", "୾ঢ়ӵ౪Ᏹཽ ࢐Ӓ౨ߨ ܒᕊցಢᙑȂमΨܥ௤ ৤ષޟᛠࣨȂߴ໌ᄇӴ౨ޟΟ၌ȂٺؐএΡ഍Ԥ૖Ψඪ"),
            ("DropCap", "䧮"),
            ("Simplified", "在世界各地的浴室和柜子里，藏着成堆的药丸、糖浆和药膏。"),
            ("Mixed", "Ελληνικά και русский текст, café naïve – “quotes” § ① ② 東京の天気は晴れです。한국어 문장 pīnyīn ǎ ǐ ǒ"),
        ];
        assert_eq!(fonts(&page), ["Caption", "Big5", "DropCap"]);
    }
}
