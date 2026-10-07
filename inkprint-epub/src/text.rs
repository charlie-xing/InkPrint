//! Turning positioned text lines into paragraphs: line joining rules for
//! CJK and Latin text, paragraph breaks, page numbers, list items.

use crate::geom::Rect;

/// One line of text on a page, from the PDF text layer or from OCR.
#[derive(Debug, Clone)]
pub struct Line {
    pub rect: Rect,
    pub text: String,
    /// Font size in points (estimated from the line height for OCR text).
    pub size: f32,
}

/// Characters that are set without spaces between them: CJK ideographs,
/// kana, hangul and the CJK / full-width punctuation blocks.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF      // radicals
        | 0x3000..=0x30FF    // CJK punctuation, hiragana, katakana
        | 0x3100..=0x31FF    // bopomofo, katakana ext
        | 0x3400..=0x4DBF    // ext A
        | 0x4E00..=0x9FFF    // unified
        | 0xAC00..=0xD7AF    // hangul
        | 0xF900..=0xFAFF    // compatibility
        | 0xFE30..=0xFE4F    // compatibility forms
        | 0xFF00..=0xFFEF    // full-width forms
        | 0x20000..=0x2FFFF) // ext B+
}

/// Joins two consecutive lines of the same paragraph: no space around CJK,
/// a hyphen at the end of a Latin line is a word break, otherwise one space.
pub fn join_lines(a: &str, b: &str) -> String {
    let a = a.trim_end();
    let b = b.trim_start();
    if a.is_empty() {
        return b.to_string();
    }
    if b.is_empty() {
        return a.to_string();
    }
    let last = a.chars().last().unwrap();
    let first = b.chars().next().unwrap();
    if last == '-' {
        let before = a.chars().rev().nth(1);
        // URLs and paths break at their own hyphens: keep those.
        let last_word = a.rsplit(char::is_whitespace).next().unwrap_or("");
        let url_like = last_word.contains('/') || last_word.contains('.') || last_word.contains('@');
        if !url_like && before.is_some_and(|c| c.is_ascii_alphabetic()) && first.is_ascii_lowercase() {
            return format!("{}{}", &a[..a.len() - 1], b);
        }
        return format!("{a}{b}");
    }
    if is_cjk(last) || is_cjk(first) {
        format!("{a}{b}")
    } else {
        format!("{a} {b}")
    }
}

/// Full-width CJK punctuation, which carries its own spacing.
fn is_cjk_punct(c: char) -> bool {
    matches!(c as u32, 0x3000..=0x303F | 0xFF01..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65)
}

/// Collapses runs of whitespace and drops spaces between two CJK
/// characters, which only appear when source line breaks became spaces.
pub fn tidy(s: &str) -> String {
    let chars: Vec<char> = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        let after_cjk_punct = i > 0 && is_cjk_punct(chars[i - 1]);
        if c == ' ' && i > 0 && i + 1 < chars.len() && (after_cjk_punct || (is_cjk(chars[i - 1]) && is_cjk(chars[i + 1]))) {
            continue;
        }
        out.push(c);
    }
    out
}

/// Whether text ends a sentence (and so may end a paragraph).
pub fn ends_sentence(s: &str) -> bool {
    matches!(
        s.trim_end().chars().last(),
        Some('。' | '！' | '？' | '.' | '!' | '?' | '：' | ':' | '；' | ';' | '"' | '”' | '’' | ')' | '）' | '」' | '』' | '…')
    )
}

/// Page numbers and "Page 3 of 10" style furniture.
pub fn is_page_number(s: &str) -> bool {
    let t: String = s.trim().trim_matches(|c: char| c == '-' || c == '–' || c == '—' || c.is_whitespace()).to_string();
    if t.is_empty() {
        return false;
    }
    if t.len() <= 4 && t.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let lower = t.to_lowercase();
    if lower.len() <= 6 && !lower.is_empty() && lower.chars().all(|c| matches!(c, 'i' | 'v' | 'x' | 'l')) {
        return true;
    }
    let digits_and = |extra: &[&str]| {
        let mut rest = lower.clone();
        for e in extra {
            rest = rest.replace(e, " ");
        }
        rest.split_whitespace().all(|w| w.chars().all(|c| c.is_ascii_digit()))
            && rest.chars().any(|c| c.is_ascii_digit())
    };
    (lower.starts_with("page") && digits_and(&["page", "of", "/"]))
        || (lower.starts_with('第') && lower.ends_with('页') && digits_and(&["第", "页", "共", "/"]))
        || (lower.len() <= 9 && digits_and(&["/"]) && lower.contains('/'))
}

/// Bullets and numbered list items.
pub fn is_list_item(s: &str) -> bool {
    let t = s.trim_start();
    let mut chars = t.chars();
    let Some(first) = chars.next() else { return false };
    if matches!(first, '•' | '·' | '●' | '○' | '■' | '□' | '▪' | '◆' | '◇' | '–' | '—' | '*' | '✓' | '➢' | '►') {
        return true;
    }
    if first == '-' && chars.next() == Some(' ') {
        return true;
    }
    // "1." "2)" "3、" "(4)" "（5）" "一、"
    let t = t.trim_start_matches(['(', '（']);
    let num: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !num.is_empty() && num.len() <= 3 {
        let next = t[num.len()..].chars().next();
        return matches!(next, Some('.' | ')' | '）' | '、')) && t[num.len()..].chars().nth(1).is_some_and(|c| !c.is_ascii_digit());
    }
    let cn: String = t.chars().take_while(|c| "一二三四五六七八九十".contains(*c)).collect();
    !cn.is_empty() && t[cn.len()..].starts_with('、')
}

/// Key for spotting running headers/footers: digits collapsed, case and
/// spacing ignored.
pub fn furniture_key(s: &str) -> String {
    let mut out = String::new();
    let mut last_digit = false;
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        if c.is_ascii_digit() {
            if !last_digit {
                out.push('#');
            }
            last_digit = true;
        } else {
            out.extend(c.to_lowercase());
            last_digit = false;
        }
    }
    out
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Merges pieces of one visual row (PDF text and OCR both split rows at
/// punctuation and wide word gaps) left to right. Rows are first clustered
/// by vertical overlap alone, then each row is cut wherever pieces are
/// further apart than a column gutter, so side-by-side columns stay apart.
/// Returns rows top to bottom.
pub fn merge_rows(lines: Vec<Line>) -> Vec<Line> {
    merge_rows_within(lines, 1.5)
}

/// [`merge_rows`] with the largest gap still bridged, in line heights.
pub fn merge_rows_within(mut lines: Vec<Line>, max_gap: f32) -> Vec<Line> {
    lines.sort_by(|a, b| a.rect.cy().total_cmp(&b.rect.cy()));
    let mut bands: Vec<(Rect, Vec<Line>)> = Vec::new();
    for l in lines {
        if let Some((band, members)) = bands.last_mut() {
            let overlap = l.rect.y1.min(band.y1) - l.rect.y0.max(band.y0);
            if overlap > 0.5 * l.rect.h().min(band.h()) {
                // Grow only by the piece's overlap with the band so one tall
                // piece cannot swallow the next row.
                let r = Rect::new(band.x0.min(l.rect.x0), band.y0, band.x1.max(l.rect.x1), band.y1);
                *band = r;
                members.push(l);
                continue;
            }
        }
        bands.push((l.rect, vec![l]));
    }
    let mut out: Vec<Line> = Vec::new();
    for (_, mut members) in bands {
        members.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
        let mut acc: Option<Line> = None;
        for l in members {
            match acc.as_mut() {
                Some(a) if l.rect.x0 - a.rect.x1 < max_gap * a.rect.h().max(l.rect.h()) => {
                    a.text = join_lines(&a.text, &l.text);
                    a.rect = a.rect.union(&l.rect);
                    a.size = a.size.max(l.size);
                }
                _ => {
                    out.extend(acc.take());
                    acc = Some(l);
                }
            }
        }
        out.extend(acc);
    }
    out.sort_by(|a, b| a.rect.y0.total_cmp(&b.rect.y0).then(a.rect.x0.total_cmp(&b.rect.x0)));
    out
}

/// A paragraph rebuilt from lines, with its dominant font size.
#[derive(Debug, Clone, PartialEq)]
pub struct Para {
    pub text: String,
    pub size: f32,
}

/// Splits the lines of one text block into paragraphs. Breaks on: a gap
/// clearly larger than the block's usual line gap, a font size change, a
/// list item, a first-line indent, or a short line that ends a sentence.
pub fn paragraphs(lines: Vec<Line>) -> Vec<Para> {
    // Within one text block, pieces side by side are always the same line.
    let rows = merge_rows_within(lines, f32::INFINITY);
    if rows.is_empty() {
        return Vec::new();
    }
    let left = rows.iter().map(|l| l.rect.x0).fold(f32::MAX, f32::min);
    let right = rows.iter().map(|l| l.rect.x1).fold(f32::MIN, f32::max);
    let lh = median(rows.iter().map(|l| l.rect.h()).collect()).max(1.0);
    let gaps: Vec<f32> = rows.windows(2).map(|w| w[1].rect.y0 - w[0].rect.y1).collect();
    let usual_gap = median(gaps.clone()).max(0.0);
    let em = lh * 0.9;

    let mut out: Vec<Para> = Vec::new();
    let mut cur: Option<Para> = None;
    for (i, row) in rows.iter().enumerate() {
        let brk = match (&cur, i) {
            (None, _) | (_, 0) => true,
            (Some(_), i) => {
                let prev = &rows[i - 1];
                let gap = gaps[i - 1];
                let size_change = (row.size - prev.size).abs() > 0.15 * prev.size.max(row.size);
                let indented = row.rect.x0 > left + 1.2 * em && prev.rect.x0 <= left + 0.5 * em;
                let prev_short = prev.rect.x1 < right - 2.5 * em && ends_sentence(&prev.text);
                gap > usual_gap * 1.6 + 0.25 * lh
                    || size_change
                    || is_list_item(&row.text)
                    || indented
                    || prev_short
            }
        };
        if brk {
            if let Some(p) = cur.take() {
                out.push(p);
            }
            cur = Some(Para { text: row.text.trim().to_string(), size: row.size });
        } else if let Some(p) = cur.as_mut() {
            p.text = join_lines(&p.text, &row.text);
        }
    }
    out.extend(cur);
    for p in out.iter_mut() {
        p.text = tidy(&p.text);
    }
    out.retain(|p| !p.text.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(y: f32, x0: f32, x1: f32, text: &str) -> Line {
        Line { rect: Rect::new(x0, y, x1, y + 20.0), text: text.into(), size: 10.0 }
    }

    #[test]
    fn joins() {
        assert_eq!(join_lines("没有数据统计、没有广", "告、也没有服务器"), "没有数据统计、没有广告、也没有服务器");
        assert_eq!(join_lines("an exam-", "ple of"), "an example of");
        assert_eq!(join_lines("well-", "Known"), "well-Known");
        assert_eq!(join_lines("核实： github.com/charlie-", "xing/InkPrint"), "核实： github.com/charlie-xing/InkPrint");
        assert_eq!(join_lines("hello", "world"), "hello world");
        assert_eq!(join_lines("使用 IPP", "protocol"), "使用 IPP protocol");
        assert_eq!(join_lines("InkPrint", "在局域网上"), "InkPrint在局域网上");
    }

    #[test]
    fn tidy_spaces() {
        assert_eq!(tidy("数据。 它没有  账号"), "数据。它没有账号");
        assert_eq!(tidy("使用 IPP  协议"), "使用 IPP 协议");
        assert_eq!(tidy("任何数据。 InkPrint 不包含"), "任何数据。InkPrint 不包含");
    }

    #[test]
    fn page_numbers() {
        for s in ["12", "- 3 -", "iv", "Page 3", "Page 3 of 10", "第 3 页", "3 / 10"] {
            assert!(is_page_number(s), "{s}");
        }
        for s in ["", "Chapter 1", "3 apples", "Pages of text"] {
            assert!(!is_page_number(s), "{s}");
        }
    }

    #[test]
    fn list_items() {
        for s in ["• one", "- two", "1. first", "2) second", "（3）third", "一、总则", "3、项目"] {
            assert!(is_list_item(s), "{s}");
        }
        for s in ["1.5 times", "2026年", "-5 degrees", "Hello"] {
            assert!(!is_list_item(s), "{s}");
        }
    }

    #[test]
    fn furniture() {
        assert_eq!(furniture_key("Page 12 of 30"), furniture_key("page 3 of 30"));
        assert_ne!(furniture_key("Intro"), furniture_key("Outro"));
    }

    #[test]
    fn paragraph_breaks() {
        let lines = vec![
            line(0.0, 0.0, 400.0, "InkPrint 不收集、不传输、不共享任何个人数据。它没有账号体系、没有数据统计、没有广"),
            line(26.0, 0.0, 300.0, "告、也没有服务器。"),
            line(60.0, 0.0, 400.0, "InkPrint 把一台 Android 设备变成局域网中的虚拟打印机。同一 Wi-Fi 下"),
            line(86.0, 0.0, 380.0, "的其他设备通过 Bonjour/mDNS 发现它"),
        ];
        let p = paragraphs(lines);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].text.ends_with("没有广告、也没有服务器。"));
        assert!(p[1].text.contains("同一 Wi-Fi 下的其他设备"));
    }

    #[test]
    fn short_sentence_end_breaks_without_gap() {
        let lines = vec![
            line(0.0, 0.0, 400.0, "The first paragraph runs the full width of the"),
            line(26.0, 0.0, 120.0, "column."),
            line(52.0, 0.0, 400.0, "Second paragraph starts right below it without"),
            line(78.0, 0.0, 400.0, "any extra spacing at all."),
        ];
        let p = paragraphs(lines);
        assert_eq!(p.len(), 2, "{p:?}");
        assert_eq!(p[0].text, "The first paragraph runs the full width of the column.");
    }

    #[test]
    fn rows_split_by_ocr_are_merged() {
        let lines = vec![
            line(0.0, 170.0, 400.0, "world"),
            line(1.0, 0.0, 150.0, "hello"),
            line(0.0, 500.0, 700.0, "other column"),
        ];
        let rows = merge_rows(lines);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "hello world");
        assert_eq!(rows[1].text, "other column");
    }
}
