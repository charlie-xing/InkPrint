//! Minimal EPUB 3 writer (with an NCX for EPUB 2 readers). Images are added
//! as pages are processed; text is written at the end.

use std::io::{Seek, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::book::{Chapter, Out, ParaClass};
use crate::Error;

pub struct Meta {
    pub title: String,
    pub lang: String,
    pub identifier: String,
    /// `CCYY-MM-DDThh:mm:ssZ`
    pub modified: String,
}

pub struct EpubWriter<W: Write + Seek> {
    zip: ZipWriter<W>,
    images: Vec<String>,
    cover: Option<String>,
}

const CSS: &str = "\
body { margin: 0 0.4em; }
p { margin: 0 0 0.6em 0; text-align: justify; }
body.cjk p.b { text-indent: 2em; }
p.li { text-indent: 0; margin-left: 1em; }
p.cap { text-align: center; font-size: 0.9em; text-indent: 0; }
h1, h2, h3 { text-align: left; page-break-after: avoid; margin: 1em 0 0.6em 0; }
h1 { font-size: 1.5em; } h2 { font-size: 1.25em; } h3 { font-size: 1.1em; }
div.fig { text-align: center; margin: 0.8em 0; page-break-inside: avoid; }
div.fig img { max-width: 100%; height: auto; }
";

fn stored() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 forbids most control characters.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

impl<W: Write + Seek> EpubWriter<W> {
    pub fn new(w: W) -> Result<Self, Error> {
        let mut zip = ZipWriter::new(w);
        // Must be the first entry, uncompressed.
        zip.start_file("mimetype", stored())?;
        zip.write_all(b"application/epub+zip")?;
        zip.start_file("META-INF/container.xml", deflated())?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#,
        )?;
        Ok(Self { zip, images: Vec::new(), cover: None })
    }

    /// Stores a JPEG and returns its href relative to the content files.
    pub fn add_image(&mut self, name: &str, jpeg: &[u8]) -> Result<String, Error> {
        let href = format!("images/{name}.jpg");
        self.zip.start_file(format!("OEBPS/{href}"), stored())?;
        self.zip.write_all(jpeg)?;
        self.images.push(href.clone());
        Ok(href)
    }

    pub fn set_cover(&mut self, jpeg: &[u8]) -> Result<(), Error> {
        let href = "images/cover.jpg".to_string();
        self.zip.start_file(format!("OEBPS/{href}"), stored())?;
        self.zip.write_all(jpeg)?;
        self.cover = Some(href);
        Ok(())
    }

    pub fn finish(mut self, meta: &Meta, chapters: &[Chapter], toc: &[(u8, String, usize, String)]) -> Result<W, Error> {
        let cjk = meta.lang.starts_with("zh") || meta.lang.starts_with("ja") || meta.lang.starts_with("ko");
        let lang = escape(&meta.lang);

        self.zip.start_file("OEBPS/style.css", deflated())?;
        self.zip.write_all(CSS.as_bytes())?;

        for (i, ch) in chapters.iter().enumerate() {
            let mut body = String::new();
            for b in &ch.blocks {
                match b {
                    Out::Heading { level, text, id } => {
                        body.push_str(&format!("<h{level} id=\"{id}\">{}</h{level}>\n", escape(text)));
                    }
                    Out::Para { text, class } => {
                        let c = match class {
                            ParaClass::Body => "b",
                            ParaClass::ListItem => "li",
                            ParaClass::Caption => "cap",
                        };
                        body.push_str(&format!("<p class=\"{c}\">{}</p>\n", escape(text)));
                    }
                    Out::Figure { href, alt } => {
                        body.push_str(&format!(
                            "<div class=\"fig\"><img src=\"{}\" alt=\"{}\"/></div>\n",
                            escape(href),
                            escape(alt)
                        ));
                    }
                }
            }
            let xhtml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\" lang=\"{lang}\">\n\
<head><meta charset=\"UTF-8\"/><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/></head>\n\
<body{}>\n{body}</body>\n</html>\n",
                escape(&ch.title),
                if cjk { " class=\"cjk\"" } else { "" },
            );
            self.zip.start_file(format!("OEBPS/ch{:03}.xhtml", i + 1), deflated())?;
            self.zip.write_all(xhtml.as_bytes())?;
        }

        // Table of contents entries: headings when there are any, otherwise
        // one entry per chapter.
        let entries: Vec<(u8, String, String)> = if toc.is_empty() {
            chapters
                .iter()
                .enumerate()
                .map(|(i, c)| (1, c.title.clone(), format!("ch{:03}.xhtml", i + 1)))
                .collect()
        } else {
            toc.iter()
                .map(|(level, text, ch, id)| (*level, text.clone(), format!("ch{:03}.xhtml#{id}", ch + 1)))
                .collect()
        };

        self.zip.start_file("OEBPS/nav.xhtml", deflated())?;
        self.zip.write_all(nav_xhtml(&lang, &meta.title, &entries).as_bytes())?;
        self.zip.start_file("OEBPS/toc.ncx", deflated())?;
        self.zip.write_all(toc_ncx(meta, &entries).as_bytes())?;
        self.zip.start_file("OEBPS/content.opf", deflated())?;
        self.zip.write_all(self.opf(meta, chapters.len()).as_bytes())?;
        Ok(self.zip.finish()?)
    }

    fn opf(&self, meta: &Meta, chapters: usize) -> String {
        let mut manifest = String::new();
        let mut spine = String::new();
        manifest.push_str("    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
        manifest.push_str("    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n");
        manifest.push_str("    <item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n");
        for i in 1..=chapters {
            manifest.push_str(&format!(
                "    <item id=\"ch{i:03}\" href=\"ch{i:03}.xhtml\" media-type=\"application/xhtml+xml\"/>\n"
            ));
            spine.push_str(&format!("    <itemref idref=\"ch{i:03}\"/>\n"));
        }
        for (i, href) in self.images.iter().enumerate() {
            manifest.push_str(&format!("    <item id=\"img{i}\" href=\"{href}\" media-type=\"image/jpeg\"/>\n"));
        }
        let mut cover_meta = String::new();
        if let Some(href) = &self.cover {
            manifest.push_str(&format!(
                "    <item id=\"cover-img\" href=\"{href}\" media-type=\"image/jpeg\" properties=\"cover-image\"/>\n"
            ));
            cover_meta.push_str("    <meta name=\"cover\" content=\"cover-img\"/>\n");
        }
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"uid\" xml:lang=\"{lang}\">\n\
  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
    <dc:identifier id=\"uid\">{id}</dc:identifier>\n\
    <dc:title>{title}</dc:title>\n\
    <dc:language>{lang}</dc:language>\n\
    <dc:creator>InkPrint</dc:creator>\n\
    <meta property=\"dcterms:modified\">{modified}</meta>\n\
{cover_meta}  </metadata>\n\
  <manifest>\n{manifest}  </manifest>\n\
  <spine toc=\"ncx\">\n{spine}  </spine>\n\
</package>\n",
            lang = escape(&meta.lang),
            id = escape(&meta.identifier),
            title = escape(&meta.title),
            modified = escape(&meta.modified),
        )
    }
}

fn nav_xhtml(lang: &str, title: &str, entries: &[(u8, String, String)]) -> String {
    // Nested <ol> following heading levels.
    let mut list = String::from("<ol>\n");
    let mut depth = 1u8;
    let mut open_li = false;
    // Start at level 1 and never skip a level, or the lists would nest
    // without a parent item.
    let min = entries.iter().map(|e| e.0).min().unwrap_or(1).max(1);
    let mut prev = 0u8;
    for (level, text, href) in entries {
        let level = (level - min + 1).min(prev + 1);
        prev = level;
        while depth < level {
            list.push_str("<ol>\n");
            depth += 1;
            open_li = false;
        }
        while depth > level {
            list.push_str("</li>\n</ol>\n");
            depth -= 1;
        }
        if open_li {
            list.push_str("</li>\n");
        }
        list.push_str(&format!("<li><a href=\"{}\">{}</a>", escape(href), escape(text)));
        open_li = true;
    }
    while depth > 1 {
        list.push_str("</li>\n</ol>\n");
        depth -= 1;
    }
    if open_li {
        list.push_str("</li>\n");
    }
    list.push_str("</ol>\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\" lang=\"{lang}\">\n\
<head><meta charset=\"UTF-8\"/><title>{t}</title></head>\n\
<body>\n<nav epub:type=\"toc\" id=\"toc\">\n<h1>{t}</h1>\n{list}</nav>\n</body>\n</html>\n",
        t = escape(title),
    )
}

fn toc_ncx(meta: &Meta, entries: &[(u8, String, String)]) -> String {
    let mut points = String::new();
    for (i, (_, text, href)) in entries.iter().enumerate() {
        points.push_str(&format!(
            "    <navPoint id=\"p{n}\" playOrder=\"{n}\"><navLabel><text>{}</text></navLabel><content src=\"{}\"/></navPoint>\n",
            escape(text),
            escape(href),
            n = i + 1
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n\
  <head><meta name=\"dtb:uid\" content=\"{}\"/></head>\n\
  <docTitle><text>{}</text></docTitle>\n\
  <navMap>\n{points}  </navMap>\n</ncx>\n",
        escape(&meta.identifier),
        escape(&meta.title),
    )
}

/// `CCYY-MM-DDThh:mm:ssZ` for a Unix timestamp.
pub fn iso8601(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    #[test]
    fn timestamp() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_791_331_200), "2026-10-07T00:00:00Z");
        assert_eq!(iso8601(951_782_400 + 3661), "2000-02-29T01:01:01Z");
    }

    #[test]
    fn writes_a_well_formed_package() {
        let mut w = EpubWriter::new(Cursor::new(Vec::new())).unwrap();
        let href = w.add_image("p1-1", b"\xff\xd8fake").unwrap();
        let chapters = vec![Chapter {
            title: "第一章".into(),
            blocks: vec![
                Out::Heading { level: 1, text: "第一章 <开始>".into(), id: "h1".into() },
                Out::Para { text: "正文 & 内容".into(), class: ParaClass::Body },
                Out::Figure { href, alt: String::new() },
            ],
        }];
        let toc = vec![(1, "第一章 <开始>".to_string(), 0, "h1".to_string())];
        let meta = Meta {
            title: "Test".into(),
            lang: "zh".into(),
            identifier: "urn:uuid:x".into(),
            modified: iso8601(0),
        };
        let bytes = w.finish(&meta, &chapters, &toc).unwrap().into_inner();

        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(z.by_index(0).unwrap().name(), "mimetype");
        assert_eq!(z.by_index(0).unwrap().compression(), CompressionMethod::Stored);
        let mut ch = String::new();
        z.by_name("OEBPS/ch001.xhtml").unwrap().read_to_string(&mut ch).unwrap();
        assert!(ch.contains("<h1 id=\"h1\">第一章 &lt;开始&gt;</h1>"));
        assert!(ch.contains("正文 &amp; 内容"));
        assert!(ch.contains("class=\"cjk\""));
        let mut opf = String::new();
        z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
        assert!(opf.contains("images/p1-1.jpg"));
        assert!(opf.contains("properties=\"nav\""));
        let mut nav = String::new();
        z.by_name("OEBPS/nav.xhtml").unwrap().read_to_string(&mut nav).unwrap();
        assert!(nav.contains("ch001.xhtml#h1"));
    }

    #[test]
    fn nav_nests_levels() {
        let e = vec![
            (1, "A".to_string(), "a".to_string()),
            (2, "A1".to_string(), "a1".to_string()),
            (1, "B".to_string(), "b".to_string()),
        ];
        let n = nav_xhtml("en", "T", &e);
        let list = &n[n.find("<ol>").unwrap()..n.rfind("</ol>").unwrap() + 5];
        assert_eq!(
            list.replace('\n', ""),
            "<ol><li><a href=\"a\">A</a><ol><li><a href=\"a1\">A1</a></li></ol></li><li><a href=\"b\">B</a></li></ol>"
        );
        // Starting below level 1 or skipping a level still nests validly.
        let skip = vec![(2, "X".to_string(), "x".to_string()), (3, "Y".to_string(), "y".to_string())];
        let n = nav_xhtml("en", "T", &skip);
        assert!(n.contains("<ol>\n<li><a href=\"x\">X</a><ol>\n<li><a href=\"y\">Y</a>"), "{n}");
    }
}
