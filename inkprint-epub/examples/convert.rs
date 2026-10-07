//! Converts a PDF on the command line, for trying the pipeline on a desktop:
//!
//! ```text
//! cargo run -p inkprint-epub --example convert -- in.pdf out.epub \
//!     --pdfium /path/libpdfium.dylib [--ort /path/libonnxruntime.dylib --models dir] [--no-ocr]
//! ```

use std::path::PathBuf;
use std::time::Instant;

struct Print;

impl inkprint_epub::Progress for Print {
    fn on_page(&self, done: u32, total: u32) {
        eprintln!("page {done}/{total}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    if args.len() < 3 {
        eprintln!("usage: convert <in.pdf> <out.epub> --pdfium <lib> [--ort <lib> --models <dir>] [--no-ocr]");
        std::process::exit(2);
    }
    let opts = inkprint_epub::Options {
        title: flag("--title").unwrap_or_default(),
        pdfium_lib: flag("--pdfium").expect("--pdfium is required"),
        ort_lib: flag("--ort").unwrap_or_default(),
        models_dir: flag("--models").map(PathBuf::from),
        ocr: !args.iter().any(|a| a == "--no-ocr"),
        threads: 4,
    };
    let t = Instant::now();
    match inkprint_epub::convert(args[1].as_ref(), args[2].as_ref(), &opts, Some(&Print)) {
        Ok(s) => eprintln!("done in {:.1}s: {s:?}", t.elapsed().as_secs_f64()),
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
