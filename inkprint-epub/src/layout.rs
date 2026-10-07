//! Page regions: from the layout model (PP-DocLayout labels) or, without
//! one, from text-line geometry. Text lines are then distributed into them.

use crate::geom::Rect;
use crate::text::{merge_rows, merge_rows_within, Line};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    DocTitle,
    Title,
    Text,
    Caption,
    Figure,
    Table,
    Formula,
    /// Running headers/footers, page numbers: dropped.
    Furniture,
}

impl Kind {
    /// PP-DocLayout class name → what it becomes in the book.
    pub fn from_label(label: &str) -> Kind {
        match label {
            "doc_title" => Kind::DocTitle,
            "paragraph_title" => Kind::Title,
            "figure_title" | "table_title" | "chart_title" => Kind::Caption,
            "image" | "chart" | "seal" => Kind::Figure,
            "table" => Kind::Table,
            "formula" => Kind::Formula,
            "header" | "footer" | "number" | "header_image" | "footer_image" | "formula_number" => {
                Kind::Furniture
            }
            // text, abstract, content, reference, footnote, aside_text, algorithm, ...
            _ => Kind::Text,
        }
    }

    pub fn is_text(self) -> bool {
        matches!(self, Kind::DocTitle | Kind::Title | Kind::Text | Kind::Caption)
    }

    /// Regions cut out of the page bitmap as pictures.
    pub fn is_picture(self) -> bool {
        matches!(self, Kind::Figure | Kind::Table | Kind::Formula)
    }
}

#[derive(Debug, Clone)]
pub struct Region {
    pub rect: Rect,
    pub kind: Kind,
    pub lines: Vec<Line>,
    /// Lies in the top or bottom margin band, where running headers live.
    pub in_margin: bool,
}

/// Share of the page height treated as header/footer margin.
const MARGIN_BAND: f32 = 0.07;

fn in_margin(r: &Rect, page: &Rect) -> bool {
    r.y1 < page.y0 + page.h() * MARGIN_BAND || r.y0 > page.y1 - page.h() * MARGIN_BAND
}

/// Distributes `lines` into the model's `regions`. Placed images the model
/// missed become figures first; lines no region claims become text regions
/// of their own; text regions that end up empty are dropped.
pub fn assign(mut regions: Vec<(Rect, Kind)>, lines: Vec<Line>, images: &[Rect], page: &Rect) -> Vec<Region> {
    // Placed images the model missed are figures too. Added before grouping
    // so screenshots side by side end up as one picture.
    let found: Vec<Rect> = regions.iter().filter(|(_, k)| k.is_picture()).map(|(r, _)| *r).collect();
    for img in images {
        let big_enough = img.area() > page.area() * 0.015;
        let whole_page = img.area() > page.area() * 0.8;
        let covered = found.iter().any(|r| img.covered_by(r) > 0.5);
        if big_enough && !whole_page && !covered {
            regions.push((*img, Kind::Figure));
        }
    }
    let regions = group_side_by_side(regions, page);
    let mut regions: Vec<Region> = regions
        .into_iter()
        .map(|(rect, kind)| {
            // The model sometimes calls a heading low on the page a footer;
            // only the margins hold running headers and footers.
            let margin = in_margin(&rect, page);
            let kind = if kind == Kind::Furniture && !margin { Kind::Text } else { kind };
            Region { rect, kind, lines: Vec::new(), in_margin: margin }
        })
        .collect();

    // A text box wrapped around other regions is the model lumping a whole
    // stretch of the page together; drop it and let its lines sort
    // themselves out.
    let rects: Vec<Rect> = regions.iter().map(|r| r.rect).collect();
    let mut i = 0;
    regions.retain(|r| {
        let me = i;
        i += 1;
        !(r.kind.is_text()
            && rects.iter().enumerate().any(|(j, o)| j != me && o.area() < r.rect.area() && o.covered_by(&r.rect) > 0.8))
    });

    // Text boxes that sit inside a picture are part of the picture.
    let pictures: Vec<Rect> = regions.iter().filter(|r| r.kind.is_picture()).map(|r| r.rect).collect();
    regions.retain(|r| !r.kind.is_text() || !pictures.iter().any(|p| r.rect.covered_by(p) > 0.8));

    let mut orphans = Vec::new();
    for line in lines {
        // Cell text often spills past a table's box; anything centred in a
        // picture belongs to it.
        let (cx, cy) = (line.rect.cx(), line.rect.cy());
        if let Some(p) = regions.iter_mut().find(|r| {
            r.kind.is_picture() && cx >= r.rect.x0 && cx <= r.rect.x1 && cy >= r.rect.y0 && cy <= r.rect.y1
        }) {
            p.lines.push(line);
            continue;
        }
        let best = regions
            .iter()
            .enumerate()
            .map(|(i, r)| (i, line.rect.covered_by(&r.rect)))
            .max_by(|a, b| {
                // Prefer pictures on ties so text drawn in a figure stays in it.
                a.1.total_cmp(&b.1).then(regions[a.0].kind.is_picture().cmp(&regions[b.0].kind.is_picture()))
            });
        match best {
            Some((i, cov)) if cov >= 0.5 => regions[i].lines.push(line),
            _ => orphans.push(line),
        }
    }
    regions.retain(|r| !r.kind.is_text() || !r.lines.is_empty());
    regions.extend(text_blocks(orphans, page));
    regions
}

/// Whether the text layer misses text the layout model sees: a quarter or
/// more of the area of its text regions holds no text-layer line. Happens
/// when part of a page was printed as outlines (macOS prints Chrome's web
/// fonts that way while the header/footer stay real text) or pasted in as
/// a picture.
pub fn text_layer_has_gaps(regions: &[(Rect, Kind)], lines: &[Line], page: &Rect) -> bool {
    let (mut total, mut empty) = (0.0f32, 0.0f32);
    for (r, k) in regions {
        if !k.is_text() {
            continue;
        }
        total += r.area();
        // Text lines cover a good share of a real text block; a stray line
        // (a title the model lumped in) doesn't make a big block "text".
        let covered: f32 = lines.iter().map(|l| l.rect.intersection_area(r)).sum();
        if covered < 0.08 * r.area() {
            empty += r.area();
        }
    }
    empty > 0.25 * total && empty > 0.01 * page.area()
}

/// Text-layer lines plus the OCR lines that don't repeat one of them.
pub fn merge_ocr(mut text: Vec<Line>, ocr: Vec<Line>) -> Vec<Line> {
    let extra: Vec<Line> = ocr
        .into_iter()
        .filter(|o| !text.iter().any(|t| o.rect.covered_by(&t.rect) > 0.3 || t.rect.covered_by(&o.rect) > 0.3))
        .collect();
    text.extend(extra);
    text
}

/// Tables found from text alone: a run of at least three rows split into
/// two or more columns whose left edges line up (rows of a single segment
/// that continues one of the columns are wrapped cell text). Two-column body
/// text — every column wide — is not a table. Returns table bounds.
pub fn detect_tables(lines: &[Line], page: &Rect) -> Vec<Rect> {
    // Cell padding can be narrower than a text column gutter.
    let segs = merge_rows_within(lines.to_vec(), 0.6);
    if segs.len() < 3 {
        return Vec::new();
    }
    // Rows of side-by-side segments.
    let mut bands: Vec<(Rect, Vec<Rect>)> = Vec::new();
    for s in segs.iter() {
        if let Some((band, members)) = bands.last_mut() {
            let overlap = s.rect.y1.min(band.y1) - s.rect.y0.max(band.y0);
            if overlap > 0.5 * s.rect.h().min(band.h()) {
                *band = band.union(&s.rect);
                members.push(s.rect);
                continue;
            }
        }
        bands.push((s.rect, vec![s.rect]));
    }
    for (_, m) in bands.iter_mut() {
        m.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    }
    let mut hs: Vec<f32> = segs.iter().map(|s| s.rect.h()).collect();
    hs.sort_by(f32::total_cmp);
    let em = hs[hs.len() / 2].max(1.0);

    let mut tables = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let mut cols: Vec<f32> = Vec::new();
    let aligned = |x: f32, cols: &[f32]| cols.iter().any(|c| (c - x).abs() < 1.5 * em);
    let finish = |run: &mut Vec<usize>, cols: &mut Vec<f32>, tables: &mut Vec<Rect>| {
        let multi = run.iter().filter(|&&i| bands[i].1.len() >= 2).count();
        if multi >= 3 && cols.len() >= 2 {
            // Median width of the segments starting in each column.
            let wide_text = cols.iter().all(|c| {
                let mut w: Vec<f32> = run
                    .iter()
                    .flat_map(|&i| bands[i].1.iter())
                    .filter(|r| (r.x0 - c).abs() < 1.5 * em)
                    .map(|r| r.w())
                    .collect();
                w.sort_by(f32::total_cmp);
                !w.is_empty() && w[w.len() / 2] > 0.3 * page.w()
            });
            if !wide_text {
                let rect = run.iter().skip(1).fold(bands[run[0]].0, |acc, &i| acc.union(&bands[i].0));
                tables.push(rect);
            }
        }
        run.clear();
        cols.clear();
    };
    for i in 0..bands.len() {
        let (rect, members) = &bands[i];
        let gap = run.last().map(|&j| rect.y0 - bands[j].0.y1).unwrap_or(0.0);
        if members.len() >= 2 {
            let fits = !run.is_empty()
                && gap < 3.0 * em
                && members.iter().any(|r| aligned(r.x0, &cols));
            if !fits {
                finish(&mut run, &mut cols, &mut tables);
            }
            for r in members {
                if !aligned(r.x0, &cols) {
                    cols.push(r.x0);
                }
            }
            run.push(i);
        } else if !run.is_empty() && gap < 1.5 * em && aligned(members[0].x0, &cols) {
            run.push(i); // wrapped cell text
        } else {
            finish(&mut run, &mut cols, &mut tables);
        }
    }
    finish(&mut run, &mut cols, &mut tables);
    tables
}

/// Pictures placed side by side (screenshots in a comparison table, a row
/// of figures) become one picture, so a reflowing reader shows them as laid
/// out instead of one per screen. A short strip of text right above or
/// below such a row — its column headings or captions, which the model may
/// even call a page header — goes into the picture too. Section titles stay
/// out.
fn group_side_by_side(regions: Vec<(Rect, Kind)>, page: &Rect) -> Vec<(Rect, Kind)> {
    let mut regions: Vec<(Rect, Kind, usize)> = regions.into_iter().map(|(r, k)| (r, k, 1)).collect();
    loop {
        let mut merged = false;
        'search: for i in 0..regions.len() {
            for j in i + 1..regions.len() {
                let (a, b) = (&regions[i], &regions[j]);
                if !a.1.is_picture() || !b.1.is_picture() {
                    continue;
                }
                let v_overlap = a.0.y1.min(b.0.y1) - a.0.y0.max(b.0.y0);
                let h_gap = (b.0.x0 - a.0.x1).max(a.0.x0 - b.0.x1);
                if v_overlap > 0.5 * a.0.h().min(b.0.h()) && h_gap < 0.08 * page.w() {
                    let kind = if a.1 == Kind::Table || b.1 == Kind::Table { Kind::Table } else { Kind::Figure };
                    let joined = (a.0.union(&b.0), kind, a.2 + b.2);
                    regions[i] = joined;
                    regions.remove(j);
                    merged = true;
                    break 'search;
                }
            }
        }
        if !merged {
            break;
        }
    }
    // Pull in the headings/captions strip of each multi-picture row.
    let rows: Vec<usize> = (0..regions.len()).filter(|&i| regions[i].2 >= 2).collect();
    for i in rows {
        let row = regions[i].0;
        let mut absorbed = row;
        regions.retain(|(r, k, n)| {
            let strip = matches!(k, Kind::Text | Kind::Caption | Kind::Furniture)
                && *n == 1
                && r.h() < 0.06 * page.h()
                && r.x0 >= row.x0 - 0.02 * page.w()
                && r.x1 <= row.x1 + 0.02 * page.w()
                && ((row.y0 - r.y1 >= -1.0 && row.y0 - r.y1 < 0.04 * page.h())
                    || (r.y0 - row.y1 >= -1.0 && r.y0 - row.y1 < 0.04 * page.h()));
            if strip {
                absorbed = absorbed.union(r);
            }
            !strip
        });
        if let Some(g) = regions.iter_mut().find(|(r, _, n)| *n >= 2 && *r == row) {
            g.0 = absorbed;
        }
    }
    regions.into_iter().map(|(r, k, _)| (r, k)).collect()
}

/// Without a layout model: blocks of text from line geometry, plus placed
/// images as figures.
pub fn heuristic(lines: Vec<Line>, images: &[Rect], page: &Rect) -> Vec<Region> {
    let tables = detect_tables(&lines, page).into_iter().map(|r| (r, Kind::Table)).collect();
    assign(tables, lines, images, page)
}

/// Model regions plus tables the model missed (found from text alignment).
pub fn with_detected_tables(mut regions: Vec<(Rect, Kind)>, lines: &[Line], page: &Rect) -> Vec<(Rect, Kind)> {
    for t in detect_tables(lines, page) {
        let known = regions.iter().any(|(r, k)| k.is_picture() && t.covered_by(r) > 0.5);
        if !known {
            regions.push((t, Kind::Table));
        }
    }
    regions
}

/// Groups loose lines into text blocks: consecutive rows that overlap
/// horizontally and are no further apart than about a line height.
fn text_blocks(lines: Vec<Line>, page: &Rect) -> Vec<Region> {
    let rows = merge_rows(lines);
    let mut blocks: Vec<Region> = Vec::new();
    for row in rows {
        let joins = blocks.iter().rposition(|b| {
            let last = b.lines.last().unwrap();
            let gap = row.rect.y0 - last.rect.y1;
            let x_overlap = row.rect.x1.min(b.rect.x1) - row.rect.x0.max(b.rect.x0);
            gap > -0.5 * row.rect.h()
                && gap < 1.2 * last.rect.h().max(row.rect.h())
                && x_overlap > 0.0
                && (row.size - last.size).abs() <= 0.15 * row.size.max(last.size)
                && in_margin(&b.rect, page) == in_margin(&row.rect, page)
        });
        match joins {
            Some(i) => {
                let b = &mut blocks[i];
                b.rect = b.rect.union(&row.rect);
                b.lines.push(row);
            }
            None => blocks.push(Region {
                rect: row.rect,
                kind: Kind::Text,
                in_margin: in_margin(&row.rect, page),
                lines: vec![row],
            }),
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Rect {
        Rect::new(0.0, 0.0, 1000.0, 1400.0)
    }

    fn line(x0: f32, y: f32, x1: f32, text: &str) -> Line {
        Line { rect: Rect::new(x0, y, x1, y + 20.0), text: text.into(), size: 10.0 }
    }

    #[test]
    fn lines_go_to_their_regions_and_strays_get_blocks() {
        let regions = vec![
            (Rect::new(90.0, 90.0, 910.0, 200.0), Kind::Text),
            (Rect::new(90.0, 300.0, 910.0, 700.0), Kind::Figure),
            (Rect::new(90.0, 800.0, 910.0, 900.0), Kind::Text), // nothing lands here
        ];
        let lines = vec![
            line(100.0, 100.0, 900.0, "a"),
            line(100.0, 126.0, 900.0, "b"),
            line(200.0, 400.0, 300.0, "label in figure"),
            line(100.0, 1000.0, 900.0, "stray"),
        ];
        let r = assign(regions, lines, &[], &page());
        assert_eq!(r.len(), 3, "{r:?}");
        assert_eq!(r[0].lines.len(), 2);
        assert_eq!(r[1].kind, Kind::Figure);
        assert_eq!(r[1].lines[0].text, "label in figure");
        assert_eq!(r[2].lines[0].text, "stray");
    }

    #[test]
    fn side_by_side_pictures_and_their_headings_become_one() {
        let regions = vec![
            (Rect::new(150.0, 60.0, 500.0, 80.0), Kind::Furniture),   // left column heading
            (Rect::new(650.0, 60.0, 900.0, 80.0), Kind::Furniture),   // right column heading
            (Rect::new(100.0, 110.0, 600.0, 400.0), Kind::Figure),
            (Rect::new(640.0, 105.0, 900.0, 410.0), Kind::Figure),
            (Rect::new(100.0, 500.0, 900.0, 530.0), Kind::Title),     // next section
        ];
        let r = assign(regions, vec![], &[], &page());
        let pictures: Vec<&Region> = r.iter().filter(|x| x.kind.is_picture()).collect();
        assert_eq!(pictures.len(), 1, "{r:?}");
        assert_eq!(pictures[0].rect, Rect::new(100.0, 60.0, 900.0, 410.0));
        // The section title is not part of it (it has no lines, so it is gone,
        // but it was not absorbed either: the picture ends at 410).

        // Same when the screenshots are placed images the model didn't see.
        let images = [Rect::new(100.0, 110.0, 600.0, 400.0), Rect::new(640.0, 105.0, 900.0, 410.0)];
        let heads = vec![(Rect::new(150.0, 60.0, 500.0, 80.0), Kind::Furniture)];
        let r = assign(heads, vec![], &images, &page());
        assert_eq!(r.iter().filter(|x| x.kind.is_picture()).count(), 1, "{r:?}");
    }

    #[test]
    fn missing_body_text_calls_for_ocr() {
        let regions = vec![
            (Rect::new(100.0, 20.0, 900.0, 60.0), Kind::Furniture),
            (Rect::new(100.0, 200.0, 900.0, 800.0), Kind::Text),
            (Rect::new(100.0, 900.0, 900.0, 950.0), Kind::Text),
        ];
        // Only the small block has text-layer lines: the big one is outlines.
        let lines = vec![line(100.0, 910.0, 800.0, "caption-like")];
        assert!(text_layer_has_gaps(&regions, &lines, &page()));
        // One stray line in the big block is not enough text for it.
        let stray = vec![line(100.0, 210.0, 800.0, "title"), line(100.0, 910.0, 800.0, "x")];
        assert!(text_layer_has_gaps(&regions, &stray, &page()));
        let full: Vec<Line> = (0..12)
            .map(|i| line(100.0, 200.0 + 50.0 * i as f32, 880.0, "body"))
            .chain([line(100.0, 910.0, 800.0, "x")])
            .collect();
        assert!(!text_layer_has_gaps(&regions, &full, &page()));
    }

    #[test]
    fn ocr_fills_in_without_duplicating() {
        let text = vec![line(100.0, 100.0, 800.0, "from text layer")];
        let ocr = vec![line(102.0, 101.0, 798.0, "from text 1ayer"), line(100.0, 300.0, 800.0, "only in pixels")];
        let merged = merge_ocr(text, ocr);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].text, "only in pixels");
    }

    #[test]
    fn container_text_boxes_are_dropped() {
        let regions = vec![
            (Rect::new(50.0, 50.0, 950.0, 1300.0), Kind::Text), // wraps everything
            (Rect::new(100.0, 400.0, 900.0, 800.0), Kind::Table),
        ];
        let lines = vec![line(100.0, 100.0, 900.0, "above"), line(150.0, 500.0, 400.0, "cell")];
        let r = assign(regions, lines, &[], &page());
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r.iter().any(|x| x.kind == Kind::Table && x.lines[0].text == "cell"));
        assert!(r.iter().any(|x| x.kind == Kind::Text && x.rect.y1 < 400.0));
    }

    #[test]
    fn footer_label_away_from_the_margin_is_text() {
        let regions = vec![(Rect::new(100.0, 1100.0, 400.0, 1130.0), Kind::Furniture)];
        let r = assign(regions, vec![line(100.0, 1105.0, 300.0, "Permissions")], &[], &page());
        assert_eq!(r[0].kind, Kind::Text);
    }

    #[test]
    fn uncovered_images_become_figures() {
        let img = Rect::new(100.0, 500.0, 600.0, 900.0);
        let full = page();
        let r = heuristic(vec![], &[img, full], &page());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, Kind::Figure);
    }

    #[test]
    fn aligned_columns_are_a_table_but_two_text_columns_are_not() {
        let mut lines = vec![line(100.0, 50.0, 900.0, "Intro paragraph across the page")];
        for (i, (k, v)) in [("INTERNET", "Run the server"), ("WAKE_LOCK", "Stay awake"), ("BOOT", "Restart")].iter().enumerate() {
            let y = 200.0 + i as f32 * 60.0;
            lines.push(line(100.0, y, 300.0, k));
            lines.push(line(500.0, y, 800.0, v));
        }
        lines.push(line(500.0, 326.0, 700.0, "wrapped cell text"));
        let t = detect_tables(&lines, &page());
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0].y0, 200.0);

        let cols: Vec<Line> = (0..6)
            .flat_map(|i| {
                let y = 100.0 + i as f32 * 26.0;
                [line(100.0, y, 480.0, "left column text"), line(520.0, y, 900.0, "right column text")]
            })
            .collect();
        assert!(detect_tables(&cols, &page()).is_empty());
    }

    #[test]
    fn heuristic_blocks_split_on_gaps_and_columns() {
        let lines = vec![
            line(100.0, 100.0, 450.0, "left 1"),
            line(100.0, 126.0, 450.0, "left 2"),
            line(550.0, 100.0, 900.0, "right 1"),
            line(100.0, 300.0, 450.0, "left after gap"),
            line(450.0, 1350.0, 550.0, "7"),
        ];
        let r = heuristic(lines, &[], &page());
        assert_eq!(r.len(), 4, "{r:?}");
        assert!(r.iter().any(|b| b.lines.len() == 2));
        assert!(r.iter().any(|b| b.in_margin && b.lines[0].text == "7"));
    }
}
