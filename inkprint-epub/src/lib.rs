//! PDF → reflowable EPUB, on device.
//!
//! Each page is read through pdfium. Pages with a text layer use it as is;
//! pages that are only pictures (scans) go through OCR when the `ocr`
//! feature and models are available, and are kept as a full-page picture
//! otherwise. A layout model (or, without one, line geometry) splits the
//! page into titles, text, captions and pictures; tables, figures and
//! formulas are cut out of the page render as images. The pages are then
//! assembled into chapters and written as EPUB 3.

pub mod book;
pub mod epub;
pub mod geom;
pub mod layout;
pub mod text;

#[cfg(feature = "ocr")]
mod engine;
#[doc(hidden)]
pub mod pdf;

use std::fs::File;
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use image::{imageops, DynamicImage, GrayImage, RgbImage};

use book::{Block, ParaClass};
use geom::Rect;
use layout::{Kind, Region};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("PDF: {0}")]
    Pdf(String),
    #[error("model: {0}")]
    Model(String),
    #[error("image: {0}")]
    Image(String),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("EPUB packaging: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("cancelled")]
    Cancelled,
}

pub struct Options {
    /// Book title; empty uses the first heading.
    pub title: String,
    /// libpdfium to bind (path, or bare soname on Android).
    pub pdfium_lib: String,
    /// libonnxruntime to load (path, or bare soname on Android).
    pub ort_lib: String,
    /// Directory holding the layout and OCR models; None runs without them.
    pub models_dir: Option<PathBuf>,
    /// Recognise text on picture-only pages.
    pub ocr: bool,
    /// ONNX Runtime intra-op threads.
    pub threads: usize,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Stats {
    pub pages: u32,
    /// Pages whose text came from OCR.
    pub ocr_pages: u32,
    /// Pages kept as one full-page picture.
    pub image_pages: u32,
    /// Whether the layout model was used.
    pub layout_model: bool,
}

pub trait Progress {
    fn on_page(&self, done: u32, total: u32);
}

static CANCEL: AtomicBool = AtomicBool::new(false);

/// Asks the running conversion to stop after the current page.
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Frees the layout/OCR models until the next conversion needs them.
pub fn release_models() {
    #[cfg(feature = "ocr")]
    engine::release();
}

/// Render resolution: 200 dpi, capped so huge pages stay affordable.
const DPI: f32 = 200.0;
const MAX_RENDER_PX: f32 = 2400.0;
/// Longest side of pictures stored in the book.
const MAX_IMAGE_PX: u32 = 1600;
/// Below this many text-layer characters a page counts as picture-only.
const MIN_TEXT_CHARS: usize = 20;

struct Page {
    blocks: Vec<Block>,
    ocr: bool,
    full_image: bool,
}

#[cfg(feature = "ocr")]
type EngineRef = std::sync::Arc<engine::Engine>;
#[cfg(not(feature = "ocr"))]
type EngineRef = ();

pub fn convert(pdf: &Path, out: &Path, opts: &Options, progress: Option<&dyn Progress>) -> Result<Stats, Error> {
    CANCEL.store(false, Ordering::SeqCst);
    let pdfium = pdf::pdfium(&opts.pdfium_lib)?;
    let doc = pdfium
        .load_pdf_from_file(pdf, None)
        .map_err(|e| Error::Pdf(format!("cannot open {}: {e}", pdf.display())))?;

    let engine = load_engine(opts);
    let total = doc.pages().len() as u32;
    let part = out.with_extension("epub.part");
    let result = (|| {
        let mut writer = epub::EpubWriter::new(BufWriter::new(File::create(&part)?))?;
        let mut stats = Stats { pages: total, layout_model: engine.is_some(), ..Default::default() };
        let mut pages = Vec::with_capacity(total as usize);
        for (i, page) in doc.pages().iter().enumerate() {
            if CANCEL.load(Ordering::SeqCst) {
                return Err(Error::Cancelled);
            }
            let p = convert_page(&page, i, engine.as_ref(), opts, &mut writer, i == 0)?;
            stats.ocr_pages += p.ocr as u32;
            stats.image_pages += p.full_image as u32;
            pages.push(p.blocks);
            if let Some(cb) = progress {
                cb.on_page(i as u32 + 1, total);
            }
        }

        let zh = is_mostly_cjk(&pages);
        let assembly = book::assemble(pages, true, zh);
        let title = if !opts.title.trim().is_empty() {
            opts.title.trim().to_string()
        } else {
            assembly
                .toc
                .first()
                .map(|t| t.1.clone())
                .unwrap_or_else(|| pdf.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let meta = epub::Meta {
            title,
            lang: if zh { "zh".into() } else { "en".into() },
            identifier: format!("urn:uuid:{}", random_uuid()),
            modified: epub::iso8601(now),
        };
        let w = writer.finish(&meta, &assembly.chapters, &assembly.toc)?;
        w.into_inner().map_err(|e| Error::Io(e.into_error()))?.sync_all()?;
        Ok(stats)
    })();

    match result {
        Ok(stats) => {
            std::fs::rename(&part, out)?;
            Ok(stats)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Debugging aid: the regions of one page, in reading order, as
/// `(bounds, kind, text)`.
#[doc(hidden)]
pub fn page_regions(pdf: &Path, index: u16, opts: &Options) -> Result<Vec<(Rect, Kind, String)>, Error> {
    let pdfium = pdf::pdfium(&opts.pdfium_lib)?;
    let doc = pdfium.load_pdf_from_file(pdf, None).map_err(|e| Error::Pdf(e.to_string()))?;
    let page = doc.pages().get(index as i32).map_err(|e| Error::Pdf(e.to_string()))?;
    let engine = load_engine(opts);
    let scale = (DPI / 72.0).min(MAX_RENDER_PX / page.width().value.max(1.0));
    let info = pdf::analyze(&page, scale);
    let bitmap = pdf::render(&page, scale)?;
    let lines = info.lines;
    let regions = match layout_regions(engine.as_ref(), &bitmap, index as usize) {
        Some(r) => {
            let r = layout::with_detected_tables(r, &lines, &info.bounds);
            layout::assign(r, lines, &info.images, &info.bounds)
        }
        None => layout::heuristic(lines, &info.images, &info.bounds),
    };
    let order = geom::xy_cut_order(&regions.iter().map(|r| r.rect).collect::<Vec<_>>());
    Ok(order
        .into_iter()
        .map(|i| {
            let r = &regions[i];
            let t = r.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" | ");
            (r.rect, r.kind, t)
        })
        .collect())
}

#[cfg(feature = "ocr")]
fn load_engine(opts: &Options) -> Option<EngineRef> {
    let dir = opts.models_dir.as_ref()?;
    match engine::get(&opts.ort_lib, dir, opts.threads) {
        Ok(e) => Some(e),
        Err(e) => {
            tracing::warn!("layout/OCR unavailable, using text-layer heuristics: {e}");
            None
        }
    }
}

#[cfg(not(feature = "ocr"))]
fn load_engine(_: &Options) -> Option<EngineRef> {
    None
}

fn convert_page<W: std::io::Write + std::io::Seek>(
    page: &pdfium_render::prelude::PdfPage,
    index: usize,
    engine: Option<&EngineRef>,
    opts: &Options,
    writer: &mut epub::EpubWriter<W>,
    cover: bool,
) -> Result<Page, Error> {
    let scale = (DPI / 72.0).min(MAX_RENDER_PX / page.width().value.max(1.0));
    let info = pdf::analyze(page, scale);
    let bitmap = pdf::render(page, scale)?;
    if cover {
        let w = 600.min(bitmap.width());
        let thumb = imageops::thumbnail(&bitmap, w, bitmap.height() * w / bitmap.width().max(1));
        writer.set_cover(&jpeg(&DynamicImage::ImageRgb8(thumb).to_luma8())?)?;
    }

    // Scans carry their text only as pixels: a big picture and next to no
    // text layer. Pages with no text layer at all may still show text drawn
    // as outlines, so they are OCR candidates too.
    let picture_area: f32 = info.images.iter().map(|r| r.area()).sum();
    let picture_only = info.chars < MIN_TEXT_CHARS && (info.chars == 0 || picture_area > 0.5 * info.bounds.area());

    let mut used_ocr = false;
    let lines = if !picture_only {
        Some(info.lines)
    } else {
        let ocr_lines = ocr_lines(engine, opts, &bitmap, scale, index);
        used_ocr = ocr_lines.as_ref().is_some_and(|l| !l.is_empty());
        ocr_lines.filter(|l| !l.is_empty())
    };

    let Some(lines) = lines else {
        // Nothing to read: keep the page as a picture, unless it is blank.
        if is_blank(&bitmap) {
            return Ok(Page { blocks: Vec::new(), ocr: false, full_image: false });
        }
        let href = writer.add_image(&format!("p{}", index + 1), &jpeg(&fit(DynamicImage::ImageRgb8(bitmap).to_luma8()))?)?;
        return Ok(Page { blocks: vec![Block::Figure { href, alt: String::new() }], ocr: false, full_image: true });
    };

    let regions = match layout_regions(engine, &bitmap, index) {
        Some(r) => {
            let r = layout::with_detected_tables(r, &lines, &info.bounds);
            layout::assign(r, lines, &info.images, &info.bounds)
        }
        None => layout::heuristic(lines, &info.images, &info.bounds),
    };
    let blocks = regions_to_blocks(regions, &bitmap, index, writer)?;
    Ok(Page { blocks, ocr: used_ocr, full_image: false })
}

#[cfg(feature = "ocr")]
fn ocr_lines(engine: Option<&EngineRef>, opts: &Options, bitmap: &RgbImage, scale: f32, index: usize) -> Option<Vec<text::Line>> {
    if !opts.ocr {
        return None;
    }
    match engine?.ocr(bitmap, scale) {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::warn!("page {}: OCR failed: {e}", index + 1);
            None
        }
    }
}

#[cfg(not(feature = "ocr"))]
fn ocr_lines(_: Option<&EngineRef>, _: &Options, _: &RgbImage, _: f32, _: usize) -> Option<Vec<text::Line>> {
    None
}

#[cfg(feature = "ocr")]
fn layout_regions(engine: Option<&EngineRef>, bitmap: &RgbImage, index: usize) -> Option<Vec<(Rect, Kind)>> {
    match engine?.layout(bitmap) {
        Ok(r) => Some(r),
        Err(e) => {
            tracing::warn!("page {}: layout analysis failed, using heuristics: {e}", index + 1);
            None
        }
    }
}

#[cfg(not(feature = "ocr"))]
fn layout_regions(_: Option<&EngineRef>, _: &RgbImage, _: usize) -> Option<Vec<(Rect, Kind)>> {
    None
}

fn regions_to_blocks<W: std::io::Write + std::io::Seek>(
    regions: Vec<Region>,
    bitmap: &RgbImage,
    index: usize,
    writer: &mut epub::EpubWriter<W>,
) -> Result<Vec<Block>, Error> {
    let rects: Vec<Rect> = regions.iter().map(|r| r.rect).collect();
    let order = geom::xy_cut_order(&rects);
    let mut regions: Vec<Option<Region>> = regions.into_iter().map(Some).collect();
    let page_rect = Rect::new(0.0, 0.0, bitmap.width() as f32, bitmap.height() as f32);
    let mut blocks = Vec::new();
    let mut figures = 0;
    for i in order {
        let r = regions[i].take().unwrap();
        match r.kind {
            Kind::Furniture => {}
            Kind::DocTitle | Kind::Title if !looks_like_body(&r) => {
                let size = r.lines.iter().map(|l| l.size).fold(0.0, f32::max);
                let text = text::merge_rows_within(r.lines, f32::INFINITY)
                    .into_iter()
                    .fold(String::new(), |acc, l| text::join_lines(&acc, &l.text));
                let text = text::tidy(&text);
                if r.in_margin {
                    blocks.push(book::margin_para(text, size));
                } else if !text.is_empty() {
                    blocks.push(Block::Heading { text, size, doc_title: r.kind == Kind::DocTitle });
                }
            }
            Kind::Text | Kind::Caption | Kind::DocTitle | Kind::Title => {
                for p in text::paragraphs(r.lines) {
                    blocks.push(if r.in_margin {
                        book::margin_para(p.text, p.size)
                    } else if r.kind == Kind::Caption {
                        Block::para(p.text, p.size, ParaClass::Caption)
                    } else if text::is_list_item(&p.text) {
                        Block::para(p.text, p.size, ParaClass::ListItem)
                    } else {
                        Block::para(p.text, p.size, ParaClass::Body)
                    });
                }
            }
            Kind::Figure | Kind::Table | Kind::Formula => {
                let crop = r.rect.padded(16.0, &page_rect);
                if crop.w() < 24.0 || crop.h() < 24.0 {
                    continue;
                }
                let img = imageops::crop_imm(
                    bitmap,
                    crop.x0 as u32,
                    crop.y0 as u32,
                    crop.w() as u32,
                    crop.h() as u32,
                )
                .to_image();
                figures += 1;
                let href = writer.add_image(
                    &format!("p{}-{}", index + 1, figures),
                    &jpeg(&fit(DynamicImage::ImageRgb8(img).to_luma8()))?,
                )?;
                // Text inside a table stays searchable as the picture's alt.
                let alt: String = text::merge_rows(r.lines)
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(300)
                    .collect();
                blocks.push(Block::Figure { href, alt });
            }
        }
    }
    Ok(blocks)
}

/// A "title" the layout model found that is really running text: too long
/// or too many lines for a heading.
fn looks_like_body(r: &Region) -> bool {
    let chars: usize = r.lines.iter().map(|l| l.text.chars().count()).sum();
    let rows = text::merge_rows_within(r.lines.clone(), f32::INFINITY).len();
    chars > 80 || rows > 3
}

/// Shrinks a picture so its longest side is at most MAX_IMAGE_PX.
fn fit(img: GrayImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= MAX_IMAGE_PX {
        return img;
    }
    let (nw, nh) = (w * MAX_IMAGE_PX / long, h * MAX_IMAGE_PX / long);
    imageops::resize(&img, nw.max(1), nh.max(1), imageops::FilterType::Triangle)
}

fn jpeg(img: &GrayImage) -> Result<Vec<u8>, Error> {
    let mut buf = Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 80);
    img.write_with_encoder(enc).map_err(|e| Error::Image(e.to_string()))?;
    Ok(buf.into_inner())
}

/// Hardly any ink on the page.
fn is_blank(img: &RgbImage) -> bool {
    let (w, h) = img.dimensions();
    let mut dark = 0u32;
    let mut seen = 0u32;
    for y in (0..h).step_by(4) {
        for x in (0..w).step_by(4) {
            let p = img.get_pixel(x, y).0;
            seen += 1;
            if (p[0] as u32 + p[1] as u32 + p[2] as u32) < 3 * 200 {
                dark += 1;
            }
        }
    }
    seen == 0 || (dark as f32) < seen as f32 * 0.001
}

fn is_mostly_cjk(pages: &[Vec<Block>]) -> bool {
    let (mut cjk, mut letters) = (0usize, 0usize);
    for b in pages.iter().flatten() {
        let t = match b {
            Block::Heading { text, .. } | Block::Para { text, .. } => text,
            Block::Figure { .. } => continue,
        };
        for c in t.chars().filter(|c| c.is_alphanumeric()) {
            letters += 1;
            cjk += text::is_cjk(c) as usize;
        }
    }
    letters > 0 && cjk * 5 >= letters
}

/// A random (v4-style) UUID without pulling in a crate for it.
fn random_uuid() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let word = || {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.finish()
    };
    let (a, b) = (word(), word());
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0x0fff,
        (b >> 48) & 0x3fff | 0x8000,
        b & 0xffff_ffff_ffff
    )
}
