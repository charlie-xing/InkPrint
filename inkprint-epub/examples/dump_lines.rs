//! Prints the text-layer lines of one page: `dump_lines <pdfium> <file.pdf> <page>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pdfium = inkprint_epub::pdf::pdfium(&a[1]).unwrap();
    let doc = pdfium.load_pdf_from_file(&a[2], None).unwrap();
    let page = doc.pages().get(a[3].parse::<i32>().unwrap()).unwrap();
    let info = inkprint_epub::pdf::analyze(&page, 200.0 / 72.0);
    for l in info.lines {
        println!("{:7.1} {:7.1} {:7.1} {:7.1} sz={:5.2} | {}", l.rect.x0, l.rect.y0, l.rect.x1, l.rect.y1, l.size, l.text);
    }
}
