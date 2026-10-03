//! Filled vector shapes: an icon drawn rather than loaded.
//!
//! A [`Path`] is an outline in its own coordinate space — a 24 x 24 view box
//! for a typical icon set — made of lines and curves, with holes where the
//! [`FillRule`] says. [`Painter::fill_path`](crate::Painter::fill_path) scales
//! it into a rect, rasterises it on the CPU into an 8-bit coverage mask at the
//! size it is shown, and draws that from the glyph atlas exactly as text is
//! drawn. So an icon:
//!
//! - is anti-aliased by its exact area coverage, curves and all — never a
//!   polygon of facets;
//! - is tinted by the colour it is drawn with, so one path serves the hover,
//!   disabled and selected states;
//! - is crisp at every DPI and canvas zoom, because it is rasterised at the
//!   pixel size it is shown at, like a glyph;
//! - is rasterised **once** per size and reused from the atlas every frame
//!   after, and shared between windows that share a font system;
//! - needs **nothing from a backend**: it arrives as glyph instances, which
//!   every renderer already draws.
//!
//! The limit is the other side of the cache: a shape that changes every frame
//! — a chart area, a morphing selection — is a cache miss every frame. Static
//! vector art is what this is for.

use crate::{Rect, Vec2};
use std::hash::{Hash, Hasher};

/// Which parts of an overlapping or self-intersecting outline are inside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FillRule {
    /// Inside wherever the outline winds around a point at all. A hole has to
    /// be drawn the other way round from its outer contour. SVG's default.
    #[default]
    NonZero,
    /// Inside wherever a ray from the point crosses the outline an odd number
    /// of times: any contour inside another is a hole, whichever way it
    /// winds. Simpler to author by hand.
    EvenOdd,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Seg {
    Move(Vec2),
    Line(Vec2),
    Quad(Vec2, Vec2),
    Cubic(Vec2, Vec2, Vec2),
    Close,
}

/// An outline to fill, in its own coordinates from `(0, 0)` to its view
/// size. Build it once and keep it: it is plain data, and its identity is what
/// lets the rasterised result be reused.
///
/// ```
/// use libgui::{Path, FillRule, Vec2};
/// // A play triangle in a 24 x 24 icon box.
/// let play = Path::new(24.0, 24.0)
///     .move_to(Vec2::new(8.0, 5.0))
///     .line_to(Vec2::new(19.0, 12.0))
///     .line_to(Vec2::new(8.0, 19.0))
///     .close();
/// # let _ = (play, FillRule::NonZero);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    view: Vec2,
    segs: Vec<Seg>,
    rule: FillRule,
}

impl Path {
    /// An empty path whose coordinates run from `(0, 0)` to `(width, height)`,
    /// the box that is scaled into the rect it is drawn in.
    pub fn new(width: f32, height: f32) -> Self {
        Self { view: Vec2::new(width, height), segs: Vec::new(), rule: FillRule::NonZero }
    }

    /// Start a new contour at `p`.
    pub fn move_to(mut self, p: Vec2) -> Self {
        self.segs.push(Seg::Move(p));
        self
    }

    pub fn line_to(mut self, p: Vec2) -> Self {
        self.segs.push(Seg::Line(p));
        self
    }

    /// A quadratic curve through control point `c` to `p`.
    pub fn quad_to(mut self, c: Vec2, p: Vec2) -> Self {
        self.segs.push(Seg::Quad(c, p));
        self
    }

    /// A cubic curve through control points `c1` and `c2` to `p`.
    pub fn cubic_to(mut self, c1: Vec2, c2: Vec2, p: Vec2) -> Self {
        self.segs.push(Seg::Cubic(c1, c2, p));
        self
    }

    /// Close the current contour back to where it started. A contour left
    /// open is closed anyway when filled — a fill has no open edges — but
    /// saying so keeps the outline honest for anything else that reads it.
    pub fn close(mut self) -> Self {
        self.segs.push(Seg::Close);
        self
    }

    pub fn fill_rule(mut self, rule: FillRule) -> Self {
        self.rule = rule;
        self
    }

    /// A closed polygon through `points`.
    pub fn polygon(width: f32, height: f32, points: &[Vec2]) -> Self {
        let mut p = Path::new(width, height);
        for (i, &pt) in points.iter().enumerate() {
            p = if i == 0 { p.move_to(pt) } else { p.line_to(pt) };
        }
        p.close()
    }

    /// A circle, drawn clockwise, from four cubic arcs.
    pub fn circle(width: f32, height: f32, centre: Vec2, radius: f32) -> Self {
        Path::new(width, height).add_circle(centre, radius, false)
    }

    /// Add a circle as its own contour — `reverse` draws it the other way
    /// round, which under [`FillRule::NonZero`] makes it a hole.
    pub fn add_circle(self, c: Vec2, r: f32, reverse: bool) -> Self {
        // The usual four-arc approximation: off by under 0.03% of the radius.
        let k = 0.552_284_8 * r;
        let (x, y) = (c.x, c.y);
        if !reverse {
            self.move_to(Vec2::new(x + r, y))
                .cubic_to(Vec2::new(x + r, y + k), Vec2::new(x + k, y + r), Vec2::new(x, y + r))
                .cubic_to(Vec2::new(x - k, y + r), Vec2::new(x - r, y + k), Vec2::new(x - r, y))
                .cubic_to(Vec2::new(x - r, y - k), Vec2::new(x - k, y - r), Vec2::new(x, y - r))
                .cubic_to(Vec2::new(x + k, y - r), Vec2::new(x + r, y - k), Vec2::new(x + r, y))
                .close()
        } else {
            self.move_to(Vec2::new(x + r, y))
                .cubic_to(Vec2::new(x + r, y - k), Vec2::new(x + k, y - r), Vec2::new(x, y - r))
                .cubic_to(Vec2::new(x - k, y - r), Vec2::new(x - r, y - k), Vec2::new(x - r, y))
                .cubic_to(Vec2::new(x - r, y + k), Vec2::new(x - k, y + r), Vec2::new(x, y + r))
                .cubic_to(Vec2::new(x + k, y + r), Vec2::new(x + r, y + k), Vec2::new(x + r, y))
                .close()
        }
    }

    /// The view box: what the path's coordinates are measured against.
    pub fn view(&self) -> Vec2 {
        self.view
    }

    pub fn is_empty(&self) -> bool {
        !self.segs.iter().any(|s| !matches!(s, Seg::Move(_) | Seg::Close))
    }

    /// The path's identity at a size, for the atlas cache. A strong hash,
    /// because a collision here draws the wrong icon rather than merely
    /// costing a probe.
    pub(crate) fn key(&self, w: u32, h: u32) -> u64 {
        // Hashed straight into the hasher rather than gathered first: this
        // runs for every icon on every frame, cache hit or not, and a frame
        // that is not changing must not allocate. A tag per segment keeps
        // `line, quad` and `quad, line` over the same points apart.
        let mut hs = crate::id::StableHasher::new();
        "fill_path".hash(&mut hs);
        (w, h, self.rule as u32, self.view.x.to_bits(), self.view.y.to_bits()).hash(&mut hs);
        for s in &self.segs {
            match *s {
                Seg::Move(p) => (0u8, p.x.to_bits(), p.y.to_bits()).hash(&mut hs),
                Seg::Line(p) => (1u8, p.x.to_bits(), p.y.to_bits()).hash(&mut hs),
                Seg::Quad(c, p) => (2u8, c.x.to_bits(), c.y.to_bits(), p.x.to_bits(), p.y.to_bits()).hash(&mut hs),
                Seg::Cubic(a, b, p) => {
                    (3u8, a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits()).hash(&mut hs);
                    (p.x.to_bits(), p.y.to_bits()).hash(&mut hs);
                }
                Seg::Close => 4u8.hash(&mut hs),
            }
        }
        hs.finish()
    }

    /// Empty this path for reuse, keeping its storage: a caller that builds
    /// an icon every frame — a C host has nowhere else to keep one — pays for
    /// the allocation once.
    pub fn reset(&mut self, width: f32, height: f32) {
        self.view = Vec2::new(width, height);
        self.segs.clear();
        self.rule = FillRule::NonZero;
    }

    /// Rasterise into `out`, a `w` x `h` coverage buffer already zeroed, with
    /// the view box stretched over the whole of it.
    pub(crate) fn rasterize(&self, w: u32, h: u32, out: &mut [u8]) {
        let (sx, sy) = (w as f32 / self.view.x.max(1e-6), h as f32 / self.view.y.max(1e-6));
        let to_px = |p: Vec2| Vec2::new(p.x * sx, p.y * sy);
        let mut acc = Accumulator::new(w, h);
        let (mut start, mut cur) = (Vec2::ZERO, Vec2::ZERO);
        let mut open = false;
        for s in &self.segs {
            match *s {
                Seg::Move(p) => {
                    if open {
                        acc.line(cur, start);
                    }
                    start = to_px(p);
                    cur = start;
                    open = true;
                }
                Seg::Line(p) => {
                    let p = to_px(p);
                    acc.line(cur, p);
                    cur = p;
                }
                Seg::Quad(c, p) => {
                    let (c, p) = (to_px(c), to_px(p));
                    let n = segments(cur, c, c, p);
                    let mut prev = cur;
                    for i in 1..=n {
                        let q = quad(cur, c, p, i as f32 / n as f32);
                        acc.line(prev, q);
                        prev = q;
                    }
                    cur = p;
                }
                Seg::Cubic(c1, c2, p) => {
                    let (c1, c2, p) = (to_px(c1), to_px(c2), to_px(p));
                    let n = segments(cur, c1, c2, p);
                    let mut prev = cur;
                    for i in 1..=n {
                        let q = cubic(cur, c1, c2, p, i as f32 / n as f32);
                        acc.line(prev, q);
                        prev = q;
                    }
                    cur = p;
                }
                Seg::Close => {
                    if open {
                        acc.line(cur, start);
                        cur = start;
                        open = false;
                    }
                }
            }
        }
        // An open contour is closed: a fill has no open edges.
        if open {
            acc.line(cur, start);
        }
        acc.resolve(self.rule, out);
    }
}

/// How many straight pieces a curve with these control points needs to stay
/// within [`TOLERANCE`] of the true curve: the flattening error of a Bézier
/// falls with the square of the piece count, and the control polygon's
/// deviation from its chord bounds the curve's.
fn segments(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> u32 {
    let dev = |p: Vec2| {
        // Distance from p to the chord a-d.
        let (vx, vy) = (d.x - a.x, d.y - a.y);
        let len = (vx * vx + vy * vy).sqrt();
        if len < 1e-6 {
            ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt()
        } else {
            ((p.x - a.x) * vy - (p.y - a.y) * vx).abs() / len
        }
    };
    let worst = dev(b).max(dev(c));
    ((worst / TOLERANCE).sqrt().ceil() as u32).clamp(1, 256)
}

/// How far, in pixels, a flattened curve may sit from the true one.
///
/// Much finer than a font rasteriser uses, deliberately. Cutting a curve into
/// chords *inscribes* it, so every convex shape shrinks: at a quarter of a
/// pixel a 25 px circle became a 24-sided polygon and lost 0.8% of its area —
/// a ring of icons visibly thinner than the same icon as an image. A mask is
/// rasterised once and cached, so the extra pieces cost almost nothing.
const TOLERANCE: f32 = 0.02;

fn quad(a: Vec2, b: Vec2, c: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    Vec2::new(u * u * a.x + 2.0 * u * t * b.x + t * t * c.x, u * u * a.y + 2.0 * u * t * b.y + t * t * c.y)
}

fn cubic(a: Vec2, b: Vec2, c: Vec2, d: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Vec2::new(w0 * a.x + w1 * b.x + w2 * c.x + w3 * d.x, w0 * a.y + w1 * b.y + w2 * c.y + w3 * d.y)
}

/// Exact area coverage by signed-area accumulation.
///
/// Each edge adds, to every cell it crosses, the signed area it sweeps to the
/// cell's right edge; a running sum along each row then turns those into the
/// winding number of every pixel, fractional exactly where an edge passes
/// through it. That fraction *is* the pixel's area coverage, so edges are
/// anti-aliased exactly rather than sampled. The same idea as a font
/// rasteriser, which is what an icon is a cousin of.
struct Accumulator {
    w: u32,
    h: u32,
    /// One row per pixel row, two cells wider than the image, so an edge on
    /// the right border writes inside its own row.
    cells: Vec<f32>,
}

impl Accumulator {
    fn new(w: u32, h: u32) -> Self {
        Self { w, h, cells: vec![0.0; ((w + 2) * h) as usize] }
    }

    fn line(&mut self, p0: Vec2, p1: Vec2) {
        if (p0.y - p1.y).abs() < f32::EPSILON || !(p0.x.is_finite() && p0.y.is_finite() && p1.x.is_finite() && p1.y.is_finite()) {
            return;
        }
        // Walk top to bottom; the direction is the edge's winding.
        let (dir, a, b) = if p0.y < p1.y { (1.0, p0, p1) } else { (-1.0, p1, p0) };
        let dxdy = (b.x - a.x) / (b.y - a.y);
        let stride = (self.w + 2) as usize;
        let wmax = self.w as f32;
        let mut x = a.x;
        // Rows outside the image contribute nothing to it.
        let y0 = a.y.max(0.0);
        let y1 = b.y.min(self.h as f32);
        if y0 >= y1 {
            return;
        }
        x += dxdy * (y0 - a.y);
        let first = y0.floor() as u32;
        let last = (y1.ceil() as u32).min(self.h);
        for row in first..last {
            let top = (row as f32).max(y0);
            let bottom = ((row + 1) as f32).min(y1);
            let dy = bottom - top;
            if dy <= 0.0 {
                continue;
            }
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            // Clamp into the image: coverage left of it lands in column 0,
            // where it still counts toward everything to its right.
            let (x0, x1) = {
                let (l, r) = if x < xnext { (x, xnext) } else { (xnext, x) };
                (l.clamp(0.0, wmax), r.clamp(0.0, wmax))
            };
            let base = row as usize * stride;
            let x0i = x0.floor();
            let x1c = x1.ceil();
            let (i0, i1) = (x0i as usize, x1c as usize);
            if i1 <= i0 + 1 {
                // Inside one cell: the area to the right of the edge's mean x.
                let xm = 0.5 * (x0 + x1) - x0i;
                self.cells[base + i0] += d * (1.0 - xm);
                self.cells[base + i0 + 1] += d * xm;
            } else {
                // Across several cells: a ramp, linear in x.
                let s = 1.0 / (x1 - x0);
                let f0 = x0 - x0i;
                let a0 = 0.5 * s * (1.0 - f0) * (1.0 - f0);
                let f1 = x1 - x1c + 1.0;
                let am = 0.5 * s * f1 * f1;
                self.cells[base + i0] += d * a0;
                if i1 == i0 + 2 {
                    self.cells[base + i0 + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - f0);
                    self.cells[base + i0 + 1] += d * (a1 - a0);
                    for i in i0 + 2..i1 - 1 {
                        self.cells[base + i] += d * s;
                    }
                    let a2 = a1 + (i1 - i0 - 3) as f32 * s;
                    self.cells[base + i1 - 1] += d * (1.0 - a2 - am);
                }
                self.cells[base + i1] += d * am;
            }
            x = xnext;
        }
    }

    /// Sum each row into winding numbers and apply the fill rule.
    fn resolve(&self, rule: FillRule, out: &mut [u8]) {
        let stride = (self.w + 2) as usize;
        for row in 0..self.h as usize {
            let mut winding = 0.0f32;
            for col in 0..self.w as usize {
                winding += self.cells[row * stride + col];
                let cover = match rule {
                    FillRule::NonZero => winding.abs().min(1.0),
                    // A triangle wave over the winding number: 0 at even, 1 at
                    // odd, and the fraction in between at an edge.
                    FillRule::EvenOdd => {
                        let m = winding.abs() % 2.0;
                        if m > 1.0 { 2.0 - m } else { m }
                    }
                };
                out[row * self.w as usize + col] = (cover * 255.0 + 0.5) as u8;
            }
        }
    }
}

/// Where a path drawn into `r` lands on the pixel grid at `scale`, and how many
/// pixels it covers: the rect is snapped so its corner sits on a pixel, which
/// is what lets one rasterisation serve every frame it is drawn in.
pub(crate) fn pixel_box(r: Rect, scale: f32) -> (Rect, u32, u32) {
    let (w, h) = ((r.w * scale).round().max(0.0), (r.h * scale).round().max(0.0));
    let (x, y) = ((r.x * scale).round() / scale, (r.y * scale).round() / scale);
    (Rect::new(x, y, w / scale, h / scale), w as u32, h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(p: &Path, w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0u8; (w * h) as usize];
        p.rasterize(w, h, &mut out);
        out
    }

    fn total(c: &[u8]) -> f32 {
        c.iter().map(|&v| v as f32 / 255.0).sum()
    }

    /// A square on the pixel grid is exactly its pixels: fully covered inside,
    /// untouched outside, no blur.
    #[test]
    fn a_pixel_aligned_square_is_exact() {
        let p = Path::polygon(10.0, 10.0, &[Vec2::new(2.0, 2.0), Vec2::new(8.0, 2.0), Vec2::new(8.0, 8.0), Vec2::new(2.0, 8.0)]);
        let c = raster(&p, 10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let inside = (2..8).contains(&x) && (2..8).contains(&y);
                assert_eq!(c[y * 10 + x], if inside { 255 } else { 0 }, "pixel ({x}, {y})");
            }
        }
    }

    /// An edge through the middle of a pixel covers half of it: coverage is
    /// area, not a sample.
    #[test]
    fn an_edge_through_a_pixel_covers_its_area() {
        let p = Path::polygon(10.0, 10.0, &[Vec2::new(2.5, 2.0), Vec2::new(8.0, 2.0), Vec2::new(8.0, 8.0), Vec2::new(2.5, 8.0)]);
        let c = raster(&p, 10, 10);
        assert!((c[5 * 10 + 2] as i32 - 128).abs() <= 1, "half-covered pixel is {}", c[5 * 10 + 2]);
        // A diagonal halves its pixels too, and the total is the true area.
        let tri = Path::polygon(10.0, 10.0, &[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)]);
        let t = raster(&tri, 10, 10);
        assert!((total(&t) - 50.0).abs() < 0.2, "a triangle of area 50 covered {}", total(&t));
    }

    /// A circle's coverage adds up to pi r squared: the curves are followed,
    /// not cut into a coarse polygon.
    #[test]
    fn a_circle_covers_its_area() {
        let p = Path::circle(64.0, 64.0, Vec2::new(32.0, 32.0), 25.0);
        let c = raster(&p, 64, 64);
        let want = std::f32::consts::PI * 25.0 * 25.0;
        assert!((total(&c) - want).abs() / want < 0.002, "circle area {} against {want}", total(&c));
    }

    /// The two rules disagree exactly where they should. A ring drawn as two
    /// circles the *same* way round: non-zero fills the middle, even-odd
    /// punches it out. Drawn opposite ways, both punch it out.
    #[test]
    fn the_fill_rules_decide_what_a_hole_is() {
        let same = Path::new(40.0, 40.0).add_circle(Vec2::new(20.0, 20.0), 16.0, false).add_circle(Vec2::new(20.0, 20.0), 8.0, false);
        let centre = 20 * 40 + 20;
        assert_eq!(raster(&same, 40, 40)[centre], 255, "non-zero should fill a same-direction inner circle");
        assert_eq!(raster(&same.clone().fill_rule(FillRule::EvenOdd), 40, 40)[centre], 0, "even-odd should punch it out");

        let opposite = Path::new(40.0, 40.0).add_circle(Vec2::new(20.0, 20.0), 16.0, false).add_circle(Vec2::new(20.0, 20.0), 8.0, true);
        assert_eq!(raster(&opposite, 40, 40)[centre], 0, "a reversed inner circle is a hole under non-zero");
        // And the ring itself is solid under both.
        let on_ring = 20 * 40 + 32;
        assert_eq!(raster(&opposite, 40, 40)[on_ring], 255);
    }

    /// Geometry outside the box is clipped, not wrapped into the next row or
    /// written past the end.
    #[test]
    fn geometry_outside_the_box_is_clipped() {
        let p = Path::polygon(10.0, 10.0, &[Vec2::new(-5.0, -5.0), Vec2::new(15.0, -5.0), Vec2::new(15.0, 15.0), Vec2::new(-5.0, 15.0)]);
        let c = raster(&p, 10, 10);
        assert!(c.iter().all(|&v| v == 255), "a shape covering the whole box should fill all of it");
        let off = Path::polygon(10.0, 10.0, &[Vec2::new(20.0, 2.0), Vec2::new(30.0, 2.0), Vec2::new(30.0, 8.0)]);
        assert!(raster(&off, 10, 10).iter().all(|&v| v == 0), "a shape entirely to the right drew something");
    }

    /// The cache key follows the shape and the size, and nothing else.
    #[test]
    fn the_key_names_the_shape_at_a_size() {
        let a = Path::circle(24.0, 24.0, Vec2::new(12.0, 12.0), 8.0);
        assert_eq!(a.key(24, 24), a.clone().key(24, 24));
        assert_ne!(a.key(24, 24), a.key(48, 48), "two sizes shared a raster");
        assert_ne!(a.key(24, 24), a.clone().fill_rule(FillRule::EvenOdd).key(24, 24), "the rule did not count");
        assert_ne!(a.key(24, 24), Path::circle(24.0, 24.0, Vec2::new(12.0, 12.0), 9.0).key(24, 24));
    }
}
