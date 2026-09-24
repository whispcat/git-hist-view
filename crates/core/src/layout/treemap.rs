#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    pub fn inset(&self, left: f32, top: f32, right: f32, bottom: f32) -> Rect {
        Rect { x: self.x + left, y: self.y + top, w: (self.w - left - right).max(0.0), h: (self.h - top - bottom).max(0.0) }
    }

    pub fn center(&self) -> Rect {
        Rect { x: self.x + self.w / 2.0, y: self.y + self.h / 2.0, w: 0.0, h: 0.0 }
    }
}

/// Worst aspect ratio of a row of `sum`-total areas laid along a side of length `side`.
fn worst(min: f64, max: f64, sum: f64, side: f64) -> f64 {
    let (s2, w2) = (sum * sum, side * side);
    (w2 * max / s2).max(s2 / (w2 * min))
}

#[derive(Clone, Copy)]
struct R64 {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl From<R64> for Rect {
    fn from(r: R64) -> Rect {
        Rect { x: r.x as f32, y: r.y as f32, w: r.w as f32, h: r.h as f32 }
    }
}

/// Squarified treemap (Bruls et al.) that keeps children in the given order instead of sorting by size.
pub fn squarify(weights: &[f64], bounds: Rect, out: &mut Vec<Rect>) {
    let start = out.len();
    out.resize(start + weights.len(), Rect { x: bounds.x, y: bounds.y, w: 0.0, h: 0.0 });
    let total: f64 = weights.iter().sum();
    if total <= 0.0 || bounds.area() <= 0.0 {
        return;
    }
    let mut rest = R64 { x: bounds.x.into(), y: bounds.y.into(), w: bounds.w.into(), h: bounds.h.into() };
    let scale = rest.w * rest.h / total;
    let areas: Vec<f64> = weights.iter().map(|&w| w * scale).collect();
    let mut i = 0;
    while i < areas.len() {
        if areas[i] <= 0.0 {
            i += 1;
            continue;
        }
        let side = rest.w.min(rest.h);
        let (mut end, mut sum, mut lo, mut hi) = (i + 1, areas[i], areas[i], areas[i]);
        let mut ratio = worst(lo, hi, sum, side);
        while let Some(&a) = areas.get(end) {
            if a <= 0.0 {
                end += 1;
                continue;
            }
            let next = worst(lo.min(a), hi.max(a), sum + a, side);
            if next > ratio {
                break;
            }
            (ratio, sum, lo, hi, end) = (next, sum + a, lo.min(a), hi.max(a), end + 1);
        }
        let horizontal = rest.w >= rest.h;
        let thickness = if horizontal { sum / rest.h } else { sum / rest.w };
        let mut offset = 0.0;
        for (k, &a) in areas[i..end].iter().enumerate() {
            let len = a / thickness;
            let r = if horizontal {
                R64 { x: rest.x, y: rest.y + offset, w: thickness, h: len }
            } else {
                R64 { x: rest.x + offset, y: rest.y, w: len, h: thickness }
            };
            out[start + i + k] = r.into();
            offset += len;
        }
        rest = if horizontal {
            R64 { x: rest.x + thickness, w: (rest.w - thickness).max(0.0), ..rest }
        } else {
            R64 { y: rest.y + thickness, h: (rest.h - thickness).max(0.0), ..rest }
        };
        i = end;
    }
}

/// Ordered split layout: halve the ordered list by weight and cut along the longer side, recursively.
/// Aspect ratios are a little worse than squarify, but a small weight change only nudges the cut
/// positions, so cells glide instead of jumping between rows during animation.
pub fn split(weights: &[f64], bounds: Rect, out: &mut Vec<Rect>) {
    let start = out.len();
    out.resize(start + weights.len(), Rect { x: bounds.x, y: bounds.y, w: 0.0, h: 0.0 });
    let mut prefix = Vec::with_capacity(weights.len() + 1);
    prefix.push(0.0);
    for &w in weights {
        prefix.push(prefix.last().unwrap() + w.max(0.0));
    }
    let r = R64 { x: bounds.x.into(), y: bounds.y.into(), w: bounds.w.into(), h: bounds.h.into() };
    split_range(&prefix, 0, weights.len(), r, &mut out[start..]);
}

fn split_range(prefix: &[f64], lo: usize, hi: usize, r: R64, out: &mut [Rect]) {
    let total = prefix[hi] - prefix[lo];
    if hi - lo == 1 || total <= 0.0 {
        if total > 0.0 {
            out[lo] = r.into();
        }
        for o in &mut out[lo..hi] {
            if total <= 0.0 {
                *o = Rect { x: r.x as f32, y: r.y as f32, w: 0.0, h: 0.0 };
            }
        }
        return;
    }
    // The cut goes where the cumulative weight is closest to half, keeping both sides non-empty.
    let half = prefix[lo] + total / 2.0;
    let mid = match prefix[lo + 1..hi].binary_search_by(|p| p.total_cmp(&half)) {
        Ok(i) | Err(i) => {
            let i = lo + 1 + i;
            let prev = i.saturating_sub(1).max(lo + 1);
            if i >= hi || (half - prefix[prev]).abs() <= (prefix[i] - half).abs() { prev } else { i }
        }
    };
    let f = (prefix[mid] - prefix[lo]) / total;
    let (a, b) = if r.w >= r.h {
        (R64 { w: r.w * f, ..r }, R64 { x: r.x + r.w * f, w: r.w * (1.0 - f), ..r })
    } else {
        (R64 { h: r.h * f, ..r }, R64 { y: r.y + r.h * f, h: r.h * (1.0 - f), ..r })
    };
    split_range(prefix, lo, mid, a, out);
    split_range(prefix, mid, hi, b, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic xorshift so property tests need no extra crates.
    fn rng(seed: u64) -> impl FnMut() -> f64 {
        let mut s = seed.max(1);
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn weights(next: &mut impl FnMut() -> f64, n: usize) -> Vec<f64> {
        // Heavy-tailed like real file sizes.
        (0..n).map(|_| (next() * 8.0).exp().floor() + 1.0).collect()
    }

    type Layout = fn(&[f64], Rect, &mut Vec<Rect>);
    const LAYOUTS: [(&str, Layout); 2] = [("squarify", squarify), ("split", split)];

    #[test]
    fn areas_are_proportional_and_disjoint() {
        for (name, layout) in LAYOUTS {
            check_areas(name, layout);
        }
    }

    fn check_areas(name: &str, layout: Layout) {
        let mut next = rng(7);
        for trial in 0..200 {
            let n = 1 + (next() * 60.0) as usize;
            let w = weights(&mut next, n);
            let bounds = Rect { x: 10.0, y: 20.0, w: 300.0 + next() as f32 * 900.0, h: 200.0 + next() as f32 * 500.0 };
            let mut out = Vec::new();
            layout(&w, bounds, &mut out);
            let total: f64 = w.iter().sum();
            for (r, &wi) in out.iter().zip(&w) {
                let expected = wi / total;
                let actual = f64::from(r.area()) / f64::from(bounds.area());
                assert!((expected - actual).abs() < 1e-4, "{name} trial {trial}: area {actual} vs {expected}");
                let eps = 1e-2;
                assert!(
                    r.x >= bounds.x - eps
                        && r.y >= bounds.y - eps
                        && r.x + r.w <= bounds.x + bounds.w + eps
                        && r.y + r.h <= bounds.y + bounds.h + eps
                );
            }
            for (i, a) in out.iter().enumerate() {
                for b in &out[i + 1..] {
                    let overlap = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
                    let overlap_y = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
                    assert!(overlap <= 1e-2 || overlap_y <= 1e-2, "{name} trial {trial}: overlap");
                }
            }
        }
    }

    #[test]
    fn order_is_preserved() {
        let mut next = rng(11);
        let w = weights(&mut next, 40);
        for (name, layout) in LAYOUTS {
            let mut out = Vec::new();
            layout(&w, Rect { x: 0.0, y: 0.0, w: 800.0, h: 500.0 }, &mut out);
            // Both layouts fill from the top-left, so a later cell never starts above and left of an earlier one.
            for pair in out.windows(2) {
                assert!(pair[1].x >= pair[0].x - 1e-3 || pair[1].y >= pair[0].y - 1e-3, "{name}");
            }
        }
    }

    /// Mean corner displacement under +-5% weight jitter, and mean aspect ratio (1 is square).
    fn stability(layout: Layout) -> (f32, f32) {
        let mut next = rng(3);
        let (mut shift, mut aspect, mut cells) = (0.0f32, 0.0f32, 0.0f32);
        for _ in 0..50 {
            let a = weights(&mut next, 30);
            let b: Vec<f64> = a.iter().map(|w| w * (1.0 + (next() - 0.5) * 0.1)).collect();
            let bounds = Rect { x: 0.0, y: 0.0, w: 1000.0, h: 600.0 };
            let (mut ra, mut rb) = (Vec::new(), Vec::new());
            layout(&a, bounds, &mut ra);
            layout(&b, bounds, &mut rb);
            for (p, q) in ra.iter().zip(&rb) {
                shift += (p.x - q.x).abs() + (p.y - q.y).abs();
                aspect += p.w.max(p.h) / p.w.min(p.h).max(1e-3);
                cells += 1.0;
            }
        }
        (shift / cells, aspect / cells)
    }

    #[test]
    fn split_is_stable_under_small_changes() {
        let (sq_shift, sq_aspect) = stability(squarify);
        let (sp_shift, sp_aspect) = stability(split);
        println!("squarify: shift {sq_shift:.1}px aspect {sq_aspect:.2}; split: shift {sp_shift:.1}px aspect {sp_aspect:.2}");
        assert!(sp_shift < sq_shift && sp_aspect < sq_aspect);
    }
}
