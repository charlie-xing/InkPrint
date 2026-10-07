//! Prints one page's regions in reading order:
//! `regions <pdfium> <file.pdf> <page> [<ort> <models>]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opts = inkprint_epub::Options {
        title: String::new(),
        pdfium_lib: a[1].clone(),
        ort_lib: a.get(4).cloned().unwrap_or_default(),
        models_dir: a.get(5).map(Into::into),
        ocr: true,
        threads: 4,
    };
    for (r, k, t) in inkprint_epub::page_regions(a[2].as_ref(), a[3].parse().unwrap(), &opts).unwrap() {
        println!("{:6.0} {:6.0} {:6.0} {:6.0} {:<9} {}", r.x0, r.y0, r.x1, r.y1, format!("{k:?}"), t.chars().take(90).collect::<String>());
    }
}
