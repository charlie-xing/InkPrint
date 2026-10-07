//! Whole-document assembly: running header/footer removal, heading levels,
//! paragraphs continued across pages, and chapter splitting.

use std::collections::HashMap;

use crate::text::{ends_sentence, furniture_key, is_list_item, join_lines};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParaClass {
    Body,
    ListItem,
    Caption,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        text: String,
        size: f32,
        doc_title: bool,
    },
    Para {
        text: String,
        size: f32,
        class: ParaClass,
        /// Set for text in the top/bottom margin: a running header/footer
        /// candidate, identified by this key.
        margin_key: Option<String>,
    },
    /// A picture already stored in the book under `href`.
    Figure { href: String, alt: String },
}

impl Block {
    pub fn para(text: String, size: f32, class: ParaClass) -> Block {
        Block::Para { text, size, class, margin_key: None }
    }
}

/// A heading with its final level, as rendered.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Heading { level: u8, text: String, id: String },
    Para { text: String, class: ParaClass },
    Figure { href: String, alt: String },
}

#[derive(Debug, Clone)]
pub struct Chapter {
    pub title: String,
    pub blocks: Vec<Out>,
}

pub struct Assembly {
    pub chapters: Vec<Chapter>,
    /// (level, text, chapter index, anchor id) for the table of contents.
    pub toc: Vec<(u8, String, usize, String)>,
}

/// Turns per-page blocks into chapters. `promote_headings` lets large-font
/// short paragraphs become headings, catching titles the layout model (if
/// any) took for text. `zh` picks the language of generated chapter titles.
pub fn assemble(mut pages: Vec<Vec<Block>>, promote_headings: bool, zh: bool) -> Assembly {
    drop_running_furniture(&mut pages);
    let body = body_size(&pages);
    if promote_headings {
        for b in pages.iter_mut().flatten() {
            if let Block::Para { text, size, class: ParaClass::Body, .. } = b {
                let short = text.chars().count() <= 60;
                if *size >= body * 1.12 && short && (!ends_sentence(text) || text.ends_with('：') || text.ends_with(':')) {
                    *b = Block::Heading { text: std::mem::take(text), size: *size, doc_title: false };
                }
            }
        }
    }
    merge_across_pages(&mut pages);
    let levels = heading_levels(&pages);

    let page_count = pages.len();
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut toc = Vec::new();
    let mut cur: Vec<Out> = Vec::new();
    let mut cur_title: Option<String> = None;
    let mut cur_start = 0usize;
    let mut next_id = 0usize;

    let close = |cur: &mut Vec<Out>, title: &mut Option<String>, start: usize, end: usize, chapters: &mut Vec<Chapter>| {
        if cur.is_empty() {
            return;
        }
        let title = title.take().unwrap_or_else(|| page_range_title(start, end, zh));
        chapters.push(Chapter { title, blocks: std::mem::take(cur) });
    };

    for (pi, page) in pages.into_iter().enumerate() {
        // Long runs without headings still get split so readers stay fast.
        if pi - cur_start >= 20 && !cur.is_empty() {
            close(&mut cur, &mut cur_title, cur_start, pi - 1, &mut chapters);
            cur_start = pi;
        }
        for b in page {
            match b {
                Block::Heading { text, size, doc_title } => {
                    let level = levels(size, doc_title);
                    let starts_chapter = level == 1 || (level == 2 && pi - cur_start >= 10);
                    if starts_chapter && !cur.is_empty() {
                        close(&mut cur, &mut cur_title, cur_start, pi.saturating_sub(1).max(cur_start), &mut chapters);
                        cur_start = pi;
                    }
                    if cur_title.is_none() && cur.is_empty() {
                        cur_title = Some(text.clone());
                    }
                    next_id += 1;
                    let id = format!("h{next_id}");
                    if level <= 2 {
                        toc.push((level, text.clone(), chapters.len(), id.clone()));
                    }
                    cur.push(Out::Heading { level, text, id });
                }
                Block::Para { text, class, .. } => cur.push(Out::Para { text, class }),
                Block::Figure { href, alt } => cur.push(Out::Figure { href, alt }),
            }
        }
    }
    close(&mut cur, &mut cur_title, cur_start, page_count.saturating_sub(1), &mut chapters);
    Assembly { chapters, toc }
}

fn page_range_title(start: usize, end: usize, zh: bool) -> String {
    match (zh, start == end) {
        (true, true) => format!("第 {} 页", start + 1),
        (true, false) => format!("第 {}–{} 页", start + 1, end + 1),
        (false, true) => format!("Page {}", start + 1),
        (false, false) => format!("Pages {}–{}", start + 1, end + 1),
    }
}

/// Margin text repeated on many pages (book title, chapter name, "Page n")
/// is running furniture. Page numbers alone are dropped wherever they sit in
/// the margin.
fn drop_running_furniture(pages: &mut [Vec<Block>]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for page in pages.iter() {
        let mut keys: Vec<&String> = page
            .iter()
            .filter_map(|b| match b {
                Block::Para { margin_key: Some(k), .. } => Some(k),
                _ => None,
            })
            .collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            *seen.entry(k.clone()).or_default() += 1;
        }
    }
    let threshold = ((pages.len() as f32) * 0.3).ceil().max(3.0) as usize;
    for page in pages.iter_mut() {
        page.retain(|b| match b {
            Block::Para { margin_key: Some(k), text, .. } => {
                seen.get(k).copied().unwrap_or(0) < threshold && !crate::text::is_page_number(text)
            }
            _ => true,
        });
    }
}

/// The body text size: the size most characters are set in.
fn body_size(pages: &[Vec<Block>]) -> f32 {
    let mut weight: HashMap<i32, usize> = HashMap::new();
    for b in pages.iter().flatten() {
        if let Block::Para { text, size, class: ParaClass::Body, .. } = b {
            *weight.entry((size * 2.0).round() as i32).or_default() += text.chars().count();
        }
    }
    weight.into_iter().max_by_key(|&(_, w)| w).map(|(s, _)| s as f32 / 2.0).unwrap_or(10.0)
}

/// A paragraph cut by a page break continues on the next page.
fn merge_across_pages(pages: &mut [Vec<Block>]) {
    for i in 1..pages.len() {
        let continues = match (pages[i - 1].last(), pages[i].first()) {
            (
                Some(Block::Para { text: prev, class: ParaClass::Body, .. }),
                Some(Block::Para { text: next, class: ParaClass::Body, .. }),
            ) => !ends_sentence(prev) && !is_list_item(next),
            _ => false,
        };
        if continues {
            let Block::Para { text: next, .. } = pages[i].remove(0) else { unreachable!() };
            if let Some(Block::Para { text, .. }) = pages[i - 1].last_mut() {
                *text = join_lines(text, &next);
            }
        }
    }
}

/// Maps heading sizes to h1–h3: the document title is h1, the remaining
/// headings rank by size (sizes within 10% count as the same level).
fn heading_levels(pages: &[Vec<Block>]) -> impl Fn(f32, bool) -> u8 {
    let mut sizes: Vec<f32> = Vec::new();
    let mut has_doc_title = false;
    for b in pages.iter().flatten() {
        if let Block::Heading { size, doc_title, .. } = b {
            if *doc_title {
                has_doc_title = true;
            } else if !sizes.iter().any(|s| (s - size).abs() <= 0.1 * s.max(*size)) {
                sizes.push(*size);
            }
        }
    }
    sizes.sort_by(|a, b| b.total_cmp(a));
    let first = if has_doc_title { 2u8 } else { 1u8 };
    move |size: f32, doc_title: bool| {
        if doc_title {
            return 1;
        }
        let rank = sizes
            .iter()
            .position(|s| (s - size).abs() <= 0.1 * s.max(size))
            .unwrap_or(sizes.len());
        (first + rank as u8).min(3)
    }
}

/// Margin key for a paragraph found in the top/bottom margin.
pub fn margin_para(text: String, size: f32) -> Block {
    let key = furniture_key(&text);
    Block::Para { text, size, class: ParaClass::Body, margin_key: Some(key) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(t: &str) -> Block {
        Block::para(t.into(), 10.0, ParaClass::Body)
    }

    #[test]
    fn running_headers_and_page_numbers_go() {
        let pages: Vec<Vec<Block>> = (1..=5)
            .map(|i| vec![margin_para("My Book".into(), 8.0), body("text."), margin_para(i.to_string(), 8.0)])
            .collect();
        let a = assemble(pages, false, false);
        let all: Vec<&Out> = a.chapters.iter().flat_map(|c| &c.blocks).collect();
        assert_eq!(all.len(), 5);
        assert!(all.iter().all(|b| matches!(b, Out::Para { text, .. } if text == "text.")));
    }

    #[test]
    fn paragraph_continues_on_next_page() {
        let pages = vec![vec![body("没有数据统计、没有广")], vec![body("告。"), body("下一段。")]];
        let a = assemble(pages, false, true);
        let texts: Vec<String> = a.chapters[0].blocks.iter().map(|b| match b {
            Out::Para { text, .. } => text.clone(),
            _ => String::new(),
        }).collect();
        assert_eq!(texts, vec!["没有数据统计、没有广告。", "下一段。"]);
    }

    #[test]
    fn headings_levels_and_chapters() {
        let h = |t: &str, s: f32, d: bool| Block::Heading { text: t.into(), size: s, doc_title: d };
        let pages = vec![
            vec![h("Book", 24.0, true), body("intro.")],
            vec![h("Part A", 16.0, false), body("a."), h("Detail", 13.0, false), body("d.")],
        ];
        let a = assemble(pages, false, false);
        assert_eq!(a.chapters.len(), 1);
        let levels: Vec<u8> = a.chapters[0].blocks.iter().filter_map(|b| match b {
            Out::Heading { level, .. } => Some(*level),
            _ => None,
        }).collect();
        assert_eq!(levels, vec![1, 2, 3]);
        assert_eq!(a.toc.len(), 2);
        assert_eq!(a.chapters[0].title, "Book");
    }

    #[test]
    fn big_short_lines_become_headings_when_promoting() {
        let pages = vec![vec![
            Block::para("Introduction".into(), 16.0, ParaClass::Body),
            body("Body text that is long enough to be the dominant size of the page."),
        ]];
        let a = assemble(pages, true, false);
        assert!(matches!(&a.chapters[0].blocks[0], Out::Heading { level: 1, text, .. } if text == "Introduction"));
    }

    #[test]
    fn no_headings_splits_every_twenty_pages() {
        let pages: Vec<Vec<Block>> = (0..45).map(|_| vec![body("x.")]).collect();
        let a = assemble(pages, false, true);
        assert_eq!(a.chapters.len(), 3);
        assert_eq!(a.chapters[0].title, "第 1–20 页");
        assert_eq!(a.chapters[2].title, "第 41–45 页");
    }
}
