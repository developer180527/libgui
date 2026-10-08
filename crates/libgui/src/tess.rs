//! Polygons to triangles, for [`crate::Painter::fill_polygon`].
//!
//! An outline and any holes become triangles, each knowing which of its edges
//! lie on the outline (anti-aliased) and which are shared with a neighbour
//! (drawn exactly, so a translucent fill has no seams). Convex outlines are a
//! fan, in linear time; anything else is ear-clipped, with holes first bridged
//! into the outline. Simple polygons only: a self-intersecting outline draws
//! something, never panics, but not necessarily what an even-odd rule would.

use crate::Vec2;

/// A corner of the merged polygon: where it is, and which contour and which
/// point of it it came from — what says whether an edge is outline.
#[derive(Clone, Copy, Debug)]
struct Pt {
    p: Vec2,
    contour: u32,
    idx: u32,
}

/// One output triangle and its outline bits (0: a→b, 1: b→c, 2: c→a).
pub(crate) type Tri = (Vec2, Vec2, Vec2, u8);

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn signed_area(pts: &[Vec2]) -> f32 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

/// Drop repeated points (including a closing copy of the first) and
/// non-finite ones.
fn clean(c: &[Vec2]) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(c.len());
    for &p in c {
        if !(p.x.is_finite() && p.y.is_finite()) {
            continue;
        }
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// Triangulate `outer` with `holes` cut out, appending to `out`.
pub(crate) fn triangulate(outer: &[Vec2], holes: &[&[Vec2]], out: &mut Vec<Tri>) {
    let mut contours: Vec<Vec<Vec2>> = Vec::with_capacity(1 + holes.len());
    let mut o = clean(outer);
    if o.len() < 3 || signed_area(&o).abs() <= 0.0 {
        return;
    }
    // The outline runs the way the contract's triangles do (positive area);
    // holes the other way.
    if signed_area(&o) < 0.0 {
        o.reverse();
    }
    contours.push(o);
    for h in holes {
        let mut h = clean(h);
        if h.len() < 3 || signed_area(&h).abs() <= 0.0 {
            continue;
        }
        if signed_area(&h) > 0.0 {
            h.reverse();
        }
        contours.push(h);
    }
    let lens: Vec<u32> = contours.iter().map(|c| c.len() as u32).collect();
    let outline = |a: Pt, b: Pt| -> bool {
        if a.contour != b.contour {
            return false;
        }
        let n = lens[a.contour as usize];
        (a.idx + 1) % n == b.idx || (b.idx + 1) % n == a.idx
    };

    // A convex outline with no holes: a fan from its first corner.
    if contours.len() == 1 && is_convex(&contours[0]) {
        let c = &contours[0];
        let n = c.len();
        for i in 1..n - 1 {
            let mut bits = 0u8;
            bits |= (i == 1) as u8; // 0→1 is outline
            bits |= 2; // i→i+1 always is
            bits |= ((i + 1 == n - 1) as u8) << 2; // last→0
            out.push((c[0], c[i], c[i + 1], bits));
        }
        return;
    }

    let mut poly: Vec<Pt> = contours[0].iter().enumerate().map(|(i, &p)| Pt { p, contour: 0, idx: i as u32 }).collect();
    // Holes, rightmost first, each joined to the outline by a bridge to a
    // corner it can see: the polygon goes out along the bridge, round the
    // hole, and back, which leaves a single outline to clip.
    let mut order: Vec<usize> = (1..contours.len()).collect();
    order.sort_by(|&a, &b| {
        let mx = |c: &Vec<Vec2>| c.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
        mx(&contours[b]).total_cmp(&mx(&contours[a]))
    });
    for h in order {
        let hole = &contours[h];
        let m = (0..hole.len()).max_by(|&a, &b| hole[a].x.total_cmp(&hole[b].x)).unwrap_or(0);
        let mp = hole[m];
        let mut best: Option<(usize, f32)> = None;
        for (i, v) in poly.iter().enumerate() {
            let d = (v.p.x - mp.x).powi(2) + (v.p.y - mp.y).powi(2);
            if best.is_some_and(|(_, bd)| bd <= d) {
                continue;
            }
            if visible(&poly, &contours[h], mp, v.p) {
                best = Some((i, d));
            }
        }
        let Some((bi, _)) = best else { continue };
        let bridge_to = poly[bi];
        let mut ring: Vec<Pt> = Vec::with_capacity(hole.len() + 2);
        for k in 0..=hole.len() {
            let j = (m + k) % hole.len();
            ring.push(Pt { p: hole[j], contour: h as u32, idx: j as u32 });
        }
        ring.push(bridge_to);
        poly.splice(bi + 1..bi + 1, ring);
    }
    ear_clip(poly, outline, out);
}

fn is_convex(c: &[Vec2]) -> bool {
    let n = c.len();
    (0..n).all(|i| cross(c[i], c[(i + 1) % n], c[(i + 2) % n]) >= 0.0)
}

/// Whether segment `a`-`b` crosses no edge of the polygon or the hole.
fn visible(poly: &[Pt], hole: &[Vec2], a: Vec2, b: Vec2) -> bool {
    let crosses = |p: Vec2, q: Vec2| -> bool {
        if p == a || p == b || q == a || q == b {
            return false;
        }
        let d1 = cross(a, b, p);
        let d2 = cross(a, b, q);
        let d3 = cross(p, q, a);
        let d4 = cross(p, q, b);
        (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) && d1 != 0.0 && d2 != 0.0
    };
    let n = poly.len();
    for i in 0..n {
        if crosses(poly[i].p, poly[(i + 1) % n].p) {
            return false;
        }
    }
    let m = hole.len();
    for i in 0..m {
        if crosses(hole[i], hole[(i + 1) % m]) {
            return false;
        }
    }
    true
}

fn in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
}

/// Clip ears off a positively wound simple polygon until one triangle is left.
fn ear_clip(mut poly: Vec<Pt>, outline: impl Fn(Pt, Pt) -> bool, out: &mut Vec<Tri>) {
    let mut stalled = 0usize;
    let mut i = 0usize;
    while poly.len() > 3 {
        let n = poly.len();
        let (ia, ib, ic) = ((i + n - 1) % n, i % n, (i + 1) % n);
        let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
        let convex = cross(a.p, b.p, c.p) > 0.0;
        let ear = convex
            && !poly.iter().enumerate().any(|(k, v)| {
                k != ia
                    && k != ib
                    && k != ic
                    && v.p != a.p
                    && v.p != b.p
                    && v.p != c.p
                    // Only a reflex corner can be inside an ear.
                    && cross(poly[(k + n - 1) % n].p, v.p, poly[(k + 1) % n].p) <= 0.0
                    && in_triangle(v.p, a.p, b.p, c.p)
            });
        // A corner with no area (collinear, or a bridge folding back on
        // itself) is cut without a triangle.
        let flat = cross(a.p, b.p, c.p) == 0.0;
        if ear || flat || stalled > n {
            if !flat && cross(a.p, b.p, c.p) > 0.0 {
                let bits = outline(a, b) as u8 | ((outline(b, c) as u8) << 1);
                // c→a is a new diagonal, never outline — unless only three
                // corners remain, handled below.
                out.push((a.p, b.p, c.p, bits));
            }
            poly.remove(ib);
            stalled = 0;
            i = if ib == 0 { 0 } else { ib - 1 };
        } else {
            stalled += 1;
            i += 1;
        }
    }
    if poly.len() == 3 {
        let (a, b, c) = (poly[0], poly[1], poly[2]);
        if cross(a.p, b.p, c.p) > 0.0 {
            let bits = outline(a, b) as u8 | ((outline(b, c) as u8) << 1) | ((outline(c, a) as u8) << 2);
            out.push((a.p, b.p, c.p, bits));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area_of(tris: &[Tri]) -> f32 {
        tris.iter().map(|&(a, b, c, _)| cross(a, b, c).abs() * 0.5).sum()
    }

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn a_convex_shape_is_a_fan_with_its_outline_marked() {
        let sq = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        let mut t = Vec::new();
        triangulate(&sq, &[], &mut t);
        assert_eq!(t.len(), 2);
        assert_eq!(area_of(&t), 100.0);
        // Four outline edges in all; the diagonal is inner in both.
        let outline: u32 = t.iter().map(|x| x.3.count_ones()).sum();
        assert_eq!(outline, 4);
    }

    #[test]
    fn a_concave_shape_keeps_its_area() {
        // An L.
        let l = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 5.0), v(5.0, 5.0), v(5.0, 20.0), v(0.0, 20.0)];
        let mut t = Vec::new();
        triangulate(&l, &[], &mut t);
        assert_eq!(t.len(), 4);
        assert!((area_of(&t) - 175.0).abs() < 1e-3, "{}", area_of(&t));
        let outline: u32 = t.iter().map(|x| x.3.count_ones()).sum();
        assert_eq!(outline, 6, "every edge of the L is outline, and no diagonal is");
    }

    #[test]
    fn a_hole_is_cut_out() {
        let outer = [v(0.0, 0.0), v(30.0, 0.0), v(30.0, 30.0), v(0.0, 30.0)];
        let hole = [v(10.0, 10.0), v(20.0, 10.0), v(20.0, 20.0), v(10.0, 20.0)];
        let mut t = Vec::new();
        triangulate(&outer, &[&hole], &mut t);
        assert!((area_of(&t) - 800.0).abs() < 1e-2, "area {}", area_of(&t));
        let outline: u32 = t.iter().map(|x| x.3.count_ones()).sum();
        assert_eq!(outline, 8, "the outline and the hole's edges, and not the bridge");
    }

    #[test]
    fn either_winding_and_junk_points_are_handled() {
        let cw = [v(0.0, 0.0), v(0.0, 10.0), v(0.0, 10.0), v(10.0, 10.0), v(10.0, 0.0), v(f32::NAN, 1.0), v(0.0, 0.0)];
        let mut t = Vec::new();
        triangulate(&cw, &[], &mut t);
        assert_eq!(area_of(&t), 100.0);
        let mut none = Vec::new();
        triangulate(&[v(0.0, 0.0), v(1.0, 1.0)], &[], &mut none);
        triangulate(&[v(0.0, 0.0), v(1.0, 1.0), v(2.0, 2.0)], &[], &mut none);
        assert!(none.is_empty());
    }

    /// A star — every other corner reflex — and a 2,000-point wobbly ring:
    /// area preserved, and no triangle wound the wrong way.
    #[test]
    fn many_reflex_corners() {
        let star: Vec<Vec2> = (0..40)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 40.0;
                let r = if i % 2 == 0 { 50.0 } else { 20.0 };
                v(a.cos() * r, a.sin() * r)
            })
            .collect();
        let mut t = Vec::new();
        triangulate(&star, &[], &mut t);
        assert!((area_of(&t) - signed_area(&star).abs()).abs() < 0.5, "{} vs {}", area_of(&t), signed_area(&star));
        assert!(t.iter().all(|&(a, b, c, _)| cross(a, b, c) > 0.0));
        let blob: Vec<Vec2> = (0..2000)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 2000.0;
                let r = 100.0 + 12.0 * (a * 9.0).sin();
                v(a.cos() * r, a.sin() * r)
            })
            .collect();
        t.clear();
        triangulate(&blob, &[], &mut t);
        assert_eq!(t.len(), 1998);
        assert!((area_of(&t) - signed_area(&blob).abs()).abs() < 2.0);
    }
}
