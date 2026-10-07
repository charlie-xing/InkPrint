//! End-to-end conversions of the fixtures. They need native libraries, so
//! they run only when these are set (otherwise they pass as skipped):
//!
//! - `INKPRINT_PDFIUM`: libpdfium (bblanchon/pdfium-binaries)
//! - `INKPRINT_ORT` + `INKPRINT_MODELS`: libonnxruntime and the model
//!   directory, for the layout/OCR tests
//!
//! `make epub-test` sets them up.

use std::io::Read;
use std::path::{Path, PathBuf};

use inkprint_epub::{convert, Options};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn options(with_models: bool) -> Option<Options> {
    let pdfium = std::env::var("INKPRINT_PDFIUM").ok()?;
    let (ort, models) = if with_models {
        (std::env::var("INKPRINT_ORT").ok()?, Some(PathBuf::from(std::env::var("INKPRINT_MODELS").ok()?)))
    } else {
        (String::new(), None)
    };
    Some(Options { title: String::new(), pdfium_lib: pdfium, ort_lib: ort, models_dir: models, ocr: true, threads: 4 })
}

/// All chapter text of a converted book, tags included.
fn chapters(epub: &Path) -> String {
    let mut z = zip::ZipArchive::new(std::fs::File::open(epub).unwrap()).unwrap();
    assert_eq!(z.by_index(0).unwrap().name(), "mimetype");
    let mut names: Vec<String> = z.file_names().filter(|n| n.contains("/ch")).map(String::from).collect();
    names.sort();
    let mut all = String::new();
    for n in names {
        z.by_name(&n).unwrap().read_to_string(&mut all).unwrap();
    }
    all
}

#[test]
fn text_layer_without_models() {
    let Some(opts) = options(false) else { return eprintln!("skipped: INKPRINT_PDFIUM not set") };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("zh.epub");
    let stats = convert(&fixture("policy-zh.pdf"), &out, &opts, None).unwrap();
    assert_eq!(stats.pages, 4);
    assert_eq!(stats.ocr_pages, 0);
    let text = chapters(&out);
    assert!(text.contains("<h1 id=\"h1\">InkPrint 隐私政策</h1>"));
    // Joined across a line break, punctuation spacing tidied.
    assert!(text.contains("没有广告、也没有服务器。所有经它处理的内容"));
    // The line-break hyphen pdfium reports as U+0002 survives in the URL.
    assert!(text.contains("github.com/charlie-xing/InkPrint"));
    assert!(!out.with_extension("epub.part").exists());
}

#[test]
fn layout_model_turns_tables_into_pictures() {
    let Some(opts) = options(true) else { return eprintln!("skipped: INKPRINT_ORT/INKPRINT_MODELS not set") };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("zh.epub");
    let stats = convert(&fixture("policy-zh.pdf"), &out, &opts, None).unwrap();
    assert!(stats.layout_model);
    let text = chapters(&out);
    assert!(text.contains("<h2 id="), "section headings found");
    assert!(text.contains("<img src=\"images/p2-"), "permission table cut out as a picture");
}

#[test]
fn scanned_page_is_ocred() {
    let Some(opts) = options(true) else { return eprintln!("skipped: INKPRINT_ORT/INKPRINT_MODELS not set") };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("scan.epub");
    let stats = convert(&fixture("scan-zh.pdf"), &out, &opts, None).unwrap();
    assert_eq!(stats.ocr_pages, 1);
    // OCR may or may not keep the space between Latin and CJK text.
    let text: String = chapters(&out).chars().filter(|c| !c.is_whitespace()).collect();
    assert!(text.contains("InkPrint在局域网上监听打印任务"), "{text}");
}

#[test]
fn scanned_page_without_ocr_stays_a_picture() {
    let Some(mut opts) = options(false) else { return eprintln!("skipped: INKPRINT_PDFIUM not set") };
    opts.ocr = false;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("scan.epub");
    let stats = convert(&fixture("scan-zh.pdf"), &out, &opts, None).unwrap();
    assert_eq!(stats.image_pages, 1);
    assert!(chapters(&out).contains("<img src=\"images/p1.jpg\""));
}

/// macOS prints Chrome's web fonts as outlines: the text layer holds only the
/// header and footer, so the body must come from OCR.
#[test]
fn outlined_text_is_recognised() {
    let Some(opts) = options(true) else { return eprintln!("skipped: INKPRINT_ORT/INKPRINT_MODELS not set") };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("web.epub");
    let stats = convert(&fixture("outlined-web-page.pdf"), &out, &opts, None).unwrap();
    assert_eq!(stats.ocr_pages, 1);
    let text = chapters(&out);
    assert!(text.contains("The story behind InkPrint"), "{text}");
    assert!(text.contains("inspired by my daughter"), "{text}");
}

/// A scanned magazine page: a title over a picture, a caption, and two
/// columns of traditional Chinese with a narrow gutter and a paragraph that
/// runs on from one column into the next.
#[test]
fn scanned_magazine_page() {
    let Some(opts) = options(true) else { return eprintln!("skipped: INKPRINT_ORT/INKPRINT_MODELS not set") };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("mag.epub");
    let stats = convert(&fixture("magazine-zh-hant.pdf"), &out, &opts, None).unwrap();
    assert_eq!(stats.ocr_pages, 1);
    let html = chapters(&out);
    let text: String = html.chars().filter(|c| !c.is_whitespace()).collect();
    // Traditional characters the tiny recognition model's dictionary lacks.
    assert!(html.contains("<h1 id=\"h1\">衝浪者說了什麼嗎？</h1>"), "{html}");
    // The two columns are read one after the other, and the paragraph
    // broken by the column change is whole again.
    assert!(text.contains("旅館一房難求，咖啡館裡整天談論著浪高與風速，連從不下水的老漁夫"), "{text}");
    let left = text.find("他說：「我一走到燈塔前").unwrap();
    let right = text.find("如今，經常聽到人們").unwrap();
    assert!(left < right, "left column first");
    // Body text, not a table cut out as a picture.
    assert!(!html.contains("<img src=\"images/p1-"), "{html}");
}

/// Text of every page as pdfium reads it (spaces dropped).
fn pdf_text(opts: &Options, pdf: &Path) -> String {
    let pdfium = inkprint_epub::pdf::pdfium(&opts.pdfium_lib).unwrap();
    let doc = pdfium.load_pdf_from_file(pdf, None).unwrap();
    let mut all = String::new();
    for page in doc.pages().iter() {
        for l in inkprint_epub::pdf::analyze(&page, 1.0).lines {
            all.push_str(&l.text);
        }
    }
    all.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A scan gets an invisible text layer readers can select and search; the
/// page looks exactly the same.
#[test]
fn scan_becomes_searchable() {
    let Some(opts) = options(true) else { return eprintln!("skipped: INKPRINT_ORT/INKPRINT_MODELS not set") };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("searchable.pdf");
    let stats = inkprint_epub::make_searchable(&fixture("magazine-zh-hant.pdf"), &out, &opts, None).unwrap();
    assert_eq!((stats.pages, stats.ocr_pages), (1, 1));
    assert!(pdf_text(&opts, &fixture("magazine-zh-hant.pdf")).is_empty());
    let text = pdf_text(&opts, &out);
    assert!(text.contains("衝浪者說了什麼嗎？"), "{text}");
    assert!(text.contains("如今，經常聽到人們"), "{text}");

    // Same pixels as the original.
    let pdfium = inkprint_epub::pdf::pdfium(&opts.pdfium_lib).unwrap();
    let render = |p: &Path| {
        let doc = pdfium.load_pdf_from_file(p, None).unwrap();
        let page = doc.pages().get(0).unwrap();
        inkprint_epub::pdf::render(&page, 0.5).unwrap()
    };
    assert!(render(&fixture("magazine-zh-hant.pdf")) == render(&out), "the text layer must not show");
}

/// A PDF with a complete text layer needs no OCR.
#[test]
fn text_pdf_needs_no_layer() {
    let Some(opts) = options(true) else { return eprintln!("skipped: INKPRINT_ORT/INKPRINT_MODELS not set") };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("policy.pdf");
    let stats = inkprint_epub::make_searchable(&fixture("policy-zh.pdf"), &out, &opts, None).unwrap();
    assert_eq!((stats.pages, stats.ocr_pages), (4, 0));
}
