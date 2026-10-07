//! Searchable PDF: the original pages untouched, plus an invisible text
//! layer with the OCR'd text of pages that have none (scans) or are missing
//! some (text printed as outlines). Readers then select, search and look up
//! that text while the page looks exactly as before.
//!
//! The layer uses a "glyphless" font, as Tesseract's PDF renderer does: a
//! Type0/Identity-H font whose character codes are the text's UTF-16 code
//! units, mapped back to Unicode by an identity ToUnicode CMap. Nothing is
//! drawn (text render mode 3), so no glyphs need embedding — no CJK font to
//! ship. Each line is stretched (Tz) to the width OCR measured, so a
//! selection lines up with the printed text.

use std::path::Path;

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use crate::geom::Rect;
use crate::text::Line;
use crate::Error;

/// Font resource name used for the layer on every page.
const FONT: &[u8] = b"InkOCR";

/// OCR'd lines of one page, in pixels of a render at `scale` px/pt.
pub struct PageText {
    /// 0-based page index.
    pub index: usize,
    pub scale: f32,
    pub lines: Vec<Line>,
}

fn pdf_err(e: lopdf::Error) -> Error {
    Error::Pdf(e.to_string())
}

/// Writes `pdf_in` with the text layers added to `pdf_out`.
pub fn write(pdf_in: &Path, pdf_out: &Path, pages: &[PageText]) -> Result<(), Error> {
    let mut doc = Document::load(pdf_in).map_err(pdf_err)?;
    let font = add_font(&mut doc);
    let page_ids = doc.get_pages();
    for p in pages.iter().filter(|p| !p.lines.is_empty()) {
        let Some(&page_id) = page_ids.get(&(p.index as u32 + 1)) else { continue };
        let bbox = page_box(&doc, page_id);
        let content = layer_content(&p.lines, p.scale, &bbox);
        add_font_resource(&mut doc, page_id, font)?;
        append_content(&mut doc, page_id, content)?;
    }
    doc.compress();
    doc.save(pdf_out).map_err(|e| Error::Io(e))?;
    Ok(())
}

/// The glyphless Type0 font, shared by all pages.
fn add_font(doc: &mut Document) -> ObjectId {
    // Every 2-byte code maps to the same Unicode value.
    let cmap = b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
1 beginbfrange\n<0000> <FFFF> <0000>\nendbfrange\n\
endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";
    let to_unicode = doc.add_object(Stream::new(Dictionary::new(), cmap.to_vec()));
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "GlyphLessFont",
        "Flags" => 5,
        "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()],
        "ItalicAngle" => 0,
        "Ascent" => 1000,
        "Descent" => 0,
        "CapHeight" => 1000,
        "StemV" => 80,
    });
    let cid_font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "GlyphLessFont",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor,
        // Every glyph one em wide; lines are stretched to fit with Tz.
        "DW" => 1000,
        "CIDToGIDMap" => "Identity",
    });
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "GlyphLessFont",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cid_font.into()],
        "ToUnicode" => to_unicode,
    })
}

/// The page's visible box in PDF units: CropBox, else MediaBox (possibly
/// inherited), else US Letter.
fn page_box(doc: &Document, page_id: ObjectId) -> Rect {
    for key in [b"CropBox".as_slice(), b"MediaBox"] {
        if let Some(arr) = inherited(doc, page_id, key).and_then(|o| o.as_array().ok().cloned()) {
            let n: Vec<f32> = arr
                .iter()
                .filter_map(|o| doc.dereference(o).ok().and_then(|(_, o)| o.as_float().ok()))
                .collect();
            if n.len() == 4 {
                return Rect::new(n[0], n[1], n[2], n[3]);
            }
        }
    }
    Rect::new(0.0, 0.0, 612.0, 792.0)
}

/// A page attribute, looked up through the Parent chain like readers do.
fn inherited<'a>(doc: &'a Document, page_id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut id = page_id;
    for _ in 0..32 {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(v) = dict.get(key) {
            return doc.dereference(v).ok().map(|(_, o)| o);
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// Invisible text for each line, placed and stretched over where OCR saw it.
fn layer_content(lines: &[Line], scale: f32, bbox: &Rect) -> Vec<u8> {
    let mut out = String::from("BT\n3 Tr\n");
    for l in lines {
        let units: Vec<u16> = l.text.encode_utf16().collect();
        if units.is_empty() {
            continue;
        }
        let w = l.rect.w() / scale;
        let h = l.rect.h() / scale;
        let size = (h * 0.85).max(1.0);
        // Glyphs are one em wide: scale them horizontally to fill the box.
        let tz = (100.0 * w / (units.len() as f32 * size)).clamp(1.0, 1000.0);
        let x = bbox.x0 + l.rect.x0 / scale;
        // Baseline a little above the box's bottom edge.
        let y = bbox.y1 - l.rect.y1 / scale + 0.15 * h;
        let hex: String = units.iter().map(|u| format!("{u:04X}")).collect();
        out.push_str(&format!(
            "/{} {size:.2} Tf\n{tz:.2} Tz\n1 0 0 1 {x:.2} {y:.2} Tm\n<{hex}> Tj\n",
            String::from_utf8_lossy(FONT)
        ));
    }
    out.push_str("ET\n");
    out.into_bytes()
}

/// Adds the layer font to the page's own Resources (copying inherited ones
/// first, so nothing the page used goes missing).
fn add_font_resource(doc: &mut Document, page_id: ObjectId, font: ObjectId) -> Result<(), Error> {
    let mut resources: Dictionary = match inherited(doc, page_id, b"Resources") {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    let mut fonts: Dictionary = match resources.get(b"Font").ok().map(|o| doc.dereference(o)) {
        Some(Ok((_, Object::Dictionary(d)))) => d.clone(),
        _ => Dictionary::new(),
    };
    fonts.set(FONT.to_vec(), font);
    resources.set("Font", fonts);
    let page = doc.get_dictionary_mut(page_id).map_err(pdf_err)?;
    page.set("Resources", resources);
    Ok(())
}

/// Appends `content` after the page's own content, which is wrapped in
/// q/Q so whatever state it leaves behind can't move the layer.
fn append_content(doc: &mut Document, page_id: ObjectId, content: Vec<u8>) -> Result<(), Error> {
    let existing: Vec<Object> = match doc.get_dictionary(page_id).map_err(pdf_err)?.get(b"Contents") {
        Ok(Object::Array(a)) => a.clone(),
        Ok(o @ Object::Reference(_)) => vec![o.clone()],
        _ => Vec::new(),
    };
    let open = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let close = doc.add_object(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()));
    let layer = doc.add_object(Stream::new(Dictionary::new(), content));
    let mut contents: Vec<Object> = vec![open.into()];
    contents.extend(existing);
    contents.push(close.into());
    contents.push(layer.into());
    doc.get_dictionary_mut(page_id).map_err(pdf_err)?.set("Contents", contents);
    Ok(())
}
