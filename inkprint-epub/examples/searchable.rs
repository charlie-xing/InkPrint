//! Adds an OCR text layer to a PDF:
//! `searchable <in.pdf> <out.pdf> <pdfium> <ort> <models>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opts = inkprint_epub::Options {
        title: String::new(),
        pdfium_lib: a[3].clone(),
        ort_lib: a[4].clone(),
        models_dir: Some(a[5].clone().into()),
        ocr: true,
        threads: 4,
    };
    let t = std::time::Instant::now();
    match inkprint_epub::make_searchable(a[1].as_ref(), a[2].as_ref(), &opts, None) {
        Ok(s) => eprintln!("done in {:.1}s: {s:?}", t.elapsed().as_secs_f64()),
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
