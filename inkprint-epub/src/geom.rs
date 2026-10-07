//! Axis-aligned rectangles in page pixel space (top-left origin, the
//! coordinate system of the rendered page bitmap).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    pub fn w(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn h(&self) -> f32 {
        self.y1 - self.y0
    }

    pub fn cx(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }

    pub fn cy(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    pub fn area(&self) -> f32 {
        self.w().max(0.0) * self.h().max(0.0)
    }

    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    pub fn intersection_area(&self, o: &Rect) -> f32 {
        let w = self.x1.min(o.x1) - self.x0.max(o.x0);
        let h = self.y1.min(o.y1) - self.y0.max(o.y0);
        if w <= 0.0 || h <= 0.0 {
            0.0
        } else {
            w * h
        }
    }

    /// Share of `self` covered by `o` (0 when `self` is empty).
    pub fn covered_by(&self, o: &Rect) -> f32 {
        let a = self.area();
        if a <= 0.0 {
            0.0
        } else {
            self.intersection_area(o) / a
        }
    }

    /// Grows the rectangle by `d` on every side, clamped to `bounds`.
    pub fn padded(&self, d: f32, bounds: &Rect) -> Rect {
        Rect {
            x0: (self.x0 - d).max(bounds.x0),
            y0: (self.y0 - d).max(bounds.y0),
            x1: (self.x1 + d).min(bounds.x1),
            y1: (self.y1 + d).min(bounds.y1),
        }
    }
}

/// Reading order for boxes on one page: recursive XY-cut. At each level the
/// boxes are split along whichever axis has the widest empty gap — rows (top
/// to bottom) or columns (left to right) — so a full-width title is cut off
/// first and two-column text then reads column by column. Returns indices
/// into `rects`.
pub fn xy_cut_order(rects: &[Rect]) -> Vec<usize> {
    let mut out = Vec::with_capacity(rects.len());
    cut(rects, (0..rects.len()).collect(), &mut out, 0);
    out
}

fn cut(rects: &[Rect], idx: Vec<usize>, out: &mut Vec<usize>, depth: u32) {
    let rows = if idx.len() > 1 { split(rects, &idx, true) } else { None };
    let cols = if idx.len() > 1 { split(rects, &idx, false) } else { None };
    let best = match (rows, cols) {
        // Real columns run most of the way down: cut them apart first, even
        // when two paragraphs happen to end at the same height in both and
        // leave a wider band across the page.
        (Some(_), Some(c)) if is_columns(rects, &c.0) => Some(c),
        (Some(r), Some(c)) => Some(if c.1 > r.1 { c } else { r }),
        (r, c) => r.or(c),
    };
    match best {
        Some((groups, _)) if depth < 64 => {
            for g in groups {
                cut(rects, g, out, depth + 1);
            }
        }
        _ => {
            // Nothing separable: top-to-bottom, then left-to-right.
            let mut idx = idx;
            idx.sort_by(|&a, &b| {
                rects[a].y0.total_cmp(&rects[b].y0).then(rects[a].x0.total_cmp(&rects[b].x0))
            });
            out.extend(idx);
        }
    }
}

/// Side-by-side groups that each span at least 60% of their combined
/// height: text columns rather than a row of things.
fn is_columns(rects: &[Rect], groups: &[Vec<usize>]) -> bool {
    let extent = |g: &Vec<usize>| {
        let y0 = g.iter().map(|&i| rects[i].y0).fold(f32::MAX, f32::min);
        let y1 = g.iter().map(|&i| rects[i].y1).fold(f32::MIN, f32::max);
        (y0, y1)
    };
    let all: Vec<(f32, f32)> = groups.iter().map(extent).collect();
    let y0 = all.iter().map(|e| e.0).fold(f32::MAX, f32::min);
    let y1 = all.iter().map(|e| e.1).fold(f32::MIN, f32::max);
    let h = (y1 - y0).max(1.0);
    all.iter().all(|e| e.1 - e.0 >= 0.6 * h)
}

/// Cuts `idx` in two at the widest empty band along y (`horizontal`) or x.
/// Returns both halves and the band's width, or None if nothing separates.
fn split(rects: &[Rect], idx: &[usize], horizontal: bool) -> Option<(Vec<Vec<usize>>, f32)> {
    let span = |i: usize| {
        let r = &rects[i];
        if horizontal { (r.y0, r.y1) } else { (r.x0, r.x1) }
    };
    let mut sorted: Vec<usize> = idx.to_vec();
    sorted.sort_by(|&a, &b| span(a).0.total_cmp(&span(b).0));
    let mut best: Option<(usize, f32)> = None; // split position, gap
    let mut end = f32::MIN;
    for (pos, &i) in sorted.iter().enumerate() {
        let (s, e) = span(i);
        // A little tolerance so boxes that merely touch still separate.
        if pos > 0 && s >= end - 1.0 {
            let gap = s - end;
            if best.is_none_or(|(_, g)| gap > g) {
                best = Some((pos, gap));
            }
        }
        end = end.max(e);
    }
    let (pos, gap) = best?;
    let rest = sorted.split_off(pos);
    Some((vec![sorted, rest], gap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_columns_read_column_by_column() {
        // Title across the top, then a left and a right column of two blocks each.
        let rects = [
            Rect::new(0.0, 0.0, 100.0, 10.0),   // 0 title
            Rect::new(55.0, 20.0, 100.0, 40.0), // 1 right top
            Rect::new(0.0, 20.0, 45.0, 40.0),   // 2 left top
            Rect::new(0.0, 45.0, 45.0, 60.0),   // 3 left bottom
            Rect::new(55.0, 45.0, 100.0, 60.0), // 4 right bottom
        ];
        assert_eq!(xy_cut_order(&rects), vec![0, 2, 3, 1, 4]);
    }

    #[test]
    fn columns_win_over_a_coincidental_row_gap() {
        // Left column: two paragraphs; right column: two paragraphs; the
        // first paragraph of each ends at the same height, leaving a band
        // across the page wider than the gutter.
        let rects = [
            Rect::new(0.0, 0.0, 45.0, 30.0),    // 0 left p1
            Rect::new(0.0, 50.0, 45.0, 100.0),  // 1 left p2
            Rect::new(48.0, 0.0, 100.0, 30.0),  // 2 right p1
            Rect::new(48.0, 50.0, 100.0, 100.0), // 3 right p2
        ];
        assert_eq!(xy_cut_order(&rects), vec![0, 1, 2, 3]);
    }

    #[test]
    fn overlapping_boxes_fall_back_to_top_down() {
        let rects = [
            Rect::new(0.0, 10.0, 60.0, 30.0),
            Rect::new(40.0, 0.0, 100.0, 20.0),
        ];
        assert_eq!(xy_cut_order(&rects), vec![1, 0]);
    }

    #[test]
    fn coverage() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 0.0, 20.0, 10.0);
        assert_eq!(a.covered_by(&b), 0.5);
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 20.0, 10.0));
    }
}
