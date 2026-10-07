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
    let text = chapters(&out);
    assert!(text.contains("InkPrint 在局域网上监听打印任务"), "{text}");
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
