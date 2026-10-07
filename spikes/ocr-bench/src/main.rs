//! P0 spike: measure PaddleOCR (via oar-ocr / ONNX Runtime) speed and memory
//! on an Android device.
//!
//! Usage:
//!   ocr-bench ocr <det.onnx> <rec.onnx> <dict.txt> <threads> <out_dir> <image>...
//!   ocr-bench structure <layout.onnx> <layout_name> <det.onnx> <rec.onnx> <dict.txt>
//!                       <slanet.onnx> <table_dict.txt> <threads> <out_dir> <image>...
//!
//! The first image is run once as a warm-up before timing. Peak RSS is read
//! from /proc/self/status (VmHWM) after each stage.

use oar_ocr::core::config::OrtSessionConfig;
use oar_ocr::oarocr::{OAROCRBuilder, OARStructureBuilder};
use oar_ocr::prelude::load_image;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn peak_rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        })
        .map(|kb| kb / 1024.0)
        .unwrap_or(f64::NAN)
}

/// Recognition batch size override (`REC_BATCH` env var).
fn rec_batch() -> Option<usize> {
    std::env::var("REC_BATCH").ok()?.parse().ok()
}

fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

/// Session options shared by every model; `MEM_PATTERN=0` disables ORT's
/// per-shape memory pattern cache (inputs vary in size page to page).
fn ort_config(threads: usize) -> OrtSessionConfig {
    let c = OrtSessionConfig::new().with_intra_threads(threads);
    match std::env::var("MEM_PATTERN").as_deref() {
        Ok("0") => c.with_memory_pattern(false),
        _ => c,
    }
}

fn stem(p: &Path) -> String {
    p.file_stem().unwrap().to_string_lossy().into_owned()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("");
    match mode {
        "ocr" => run_ocr(&args[2..]),
        "structure" => run_structure(&args[2..]),
        "rec" => run_rec(&args[2..]),
        "pdf" => run_pdf(&args[2..]),
        _ => {
            eprintln!("usage: ocr-bench ocr|structure ...");
            std::process::exit(2);
        }
    }
}

fn run_ocr(a: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (det, rec, dict) = (&a[0], &a[1], &a[2]);
    let threads: usize = a[3].parse()?;
    let out = PathBuf::from(&a[4]);
    let images: Vec<PathBuf> = a[5..].iter().map(PathBuf::from).collect();
    std::fs::create_dir_all(&out)?;

    let t = Instant::now();
    let mut b = OAROCRBuilder::new(det.as_str(), rec.as_str(), dict.as_str())
        .ort_session(ort_config(threads));
    if let Some(n) = rec_batch() {
        b = b.region_batch_size(n);
    }
    let ocr = b.build()?;
    println!("load_ms={:.0} rss_mb={:.0}", t.elapsed().as_secs_f64() * 1e3, peak_rss_mb());

    ocr.predict(vec![load_image(&images[0])?])?;
    println!("warmup_done rss_mb={:.0}", peak_rss_mb());

    let mut total = 0.0;
    for p in &images {
        let img = load_image(p)?;
        let t = Instant::now();
        let res = ocr.predict(vec![img])?;
        let ms = t.elapsed().as_secs_f64() * 1e3;
        total += ms;
        let mut text = String::new();
        let mut chars = 0;
        for r in &res[0].text_regions {
            if let Some((s, _)) = r.text_with_confidence() {
                chars += s.chars().count();
                text.push_str(s);
                text.push('\n');
            }
        }
        std::fs::write(out.join(format!("{}.txt", stem(p))), text)?;
        println!(
            "page={} ms={:.0} regions={} chars={} rss_mb={:.0}",
            stem(p),
            ms,
            res[0].text_regions.len(),
            chars,
            peak_rss_mb()
        );
    }
    println!("avg_ms={:.0}", total / images.len() as f64);
    Ok(())
}

fn run_structure(a: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (layout, layout_name, det, rec, dict, slanet, tdict) =
        (&a[0], &a[1], &a[2], &a[3], &a[4], &a[5], &a[6]);
    let threads: usize = a[7].parse()?;
    let out = PathBuf::from(&a[8]);
    let images: Vec<PathBuf> = a[9..].iter().map(PathBuf::from).collect();
    std::fs::create_dir_all(&out)?;

    let t = Instant::now();
    let mut s = OARStructureBuilder::new(layout.as_str())
        .layout_model_name(layout_name.as_str())
        .ort_session(ort_config(threads));
    if !flag("NO_TABLE") {
        s = s
            .with_wireless_table_structure(slanet.as_str())
            .table_structure_dict_path(tdict.as_str());
    }
    if !flag("NO_OCR") {
        s = s.with_ocr(det.as_str(), rec.as_str(), dict.as_str());
    }
    let s = match rec_batch() {
        Some(n) => s.region_batch_size(n),
        None => s,
    }
    .build()?;
    println!("load_ms={:.0} rss_mb={:.0}", t.elapsed().as_secs_f64() * 1e3, peak_rss_mb());

    let _ = s.predict_images(vec![load_image(&images[0])?]);
    println!("warmup_done rss_mb={:.0}", peak_rss_mb());

    let mut total = 0.0;
    for p in &images {
        let img = load_image(p)?;
        let t = Instant::now();
        let mut res = s.predict_images(vec![img]);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        total += ms;
        let r = res.remove(0)?;
        let mut labels: Vec<String> = r
            .layout_elements
            .iter()
            .map(|e| e.label.clone().unwrap_or_else(|| e.element_type.as_str().to_string()))
            .collect();
        labels.sort();
        labels.dedup();
        std::fs::write(out.join(format!("{}.md", stem(p))), r.to_markdown())?;
        println!(
            "page={} ms={:.0} elements={} tables={} kinds=[{}] rss_mb={:.0}",
            stem(p),
            ms,
            r.layout_elements.len(),
            r.tables.len(),
            labels.join(","),
            peak_rss_mb()
        );
    }
    println!("avg_ms={:.0}", total / images.len() as f64);
    Ok(())
}

/// Recognition only, on pre-cropped line images: `rec <rec.onnx> <dict.txt> <image>...`
fn run_rec(a: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use oar_ocr::predictors::TextRecognitionPredictor;
    let p = TextRecognitionPredictor::builder()
        .dict_path(&a[1])
        .build(a[0].as_str())?;
    let imgs = a[2..].iter().map(|f| load_image(Path::new(f))).collect::<Result<Vec<_>, _>>()?;
    let out = p.predict(imgs)?;
    for t in &out.texts {
        println!("{t}");
    }
    Ok(())
}

/// Text-layer extraction and page rendering with pdfium:
/// `pdf <libpdfium dir> <dpi> <out_dir> <file.pdf>...`
///
/// Per page: extract the text and every char's bounds (what layout rebuilding
/// needs), count image objects, then render to a bitmap at `dpi`.
fn run_pdf(a: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use pdfium_render::prelude::*;
    let pdfium = Pdfium::new(Pdfium::bind_to_library(
        Pdfium::pdfium_platform_library_name_at_path(&a[0]),
    )?);
    let dpi: f32 = a[1].parse()?;
    let out = PathBuf::from(&a[2]);
    std::fs::create_dir_all(&out)?;
    for f in &a[3..] {
        let t = Instant::now();
        let doc = pdfium.load_pdf_from_file(f, None)?;
        let open_ms = t.elapsed().as_secs_f64() * 1e3;
        let (mut text_ms, mut render_ms, mut chars, mut images) = (0.0, 0.0, 0usize, 0usize);
        let n = doc.pages().len();
        for (i, page) in doc.pages().iter().enumerate() {
            let t = Instant::now();
            let text = page.text()?;
            let mut s = String::new();
            for c in text.chars().iter() {
                if let Some(ch) = c.unicode_char() {
                    let _ = c.loose_bounds()?;
                    s.push(ch);
                    chars += 1;
                }
            }
            images += page
                .objects()
                .iter()
                .filter(|o| o.as_image_object().is_some())
                .count();
            text_ms += t.elapsed().as_secs_f64() * 1e3;

            let t = Instant::now();
            let px = (page.width().value / 72.0 * dpi) as i32;
            let bmp = page.render_with_config(&PdfRenderConfig::new().set_target_width(px))?;
            let img = bmp.as_image()?.to_rgb8();
            render_ms += t.elapsed().as_secs_f64() * 1e3;
            if i == 0 {
                std::fs::write(out.join(format!("{}-p1.txt", stem(Path::new(f)))), &s)?;
                println!("  first page rendered {}x{}", img.width(), img.height());
            }
        }
        println!(
            "pdf={} pages={} open_ms={:.0} text_ms_per_page={:.1} render_ms_per_page={:.0} chars={} images={} rss_mb={:.0}",
            stem(Path::new(f)),
            n,
            open_ms,
            text_ms / n as f64,
            render_ms / n as f64,
            chars,
            images,
            peak_rss_mb()
        );
    }
    Ok(())
}
