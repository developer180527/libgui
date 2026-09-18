//! Link routing and hit-testing. Routing produces points; everything else
//! (drawing, picking) works on those points, so a new route needs no other
//! changes.

use crate::style::Routing;
use libgui::Vec2;

fn cubic(p0: Vec2, c0: Vec2, c1: Vec2, p1: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Vec2::new(p0.x * a + c0.x * b + c1.x * c + p1.x * d, p0.y * a + c0.y * b + c1.y * c + p1.y * d)
}

/// The polyline a link is drawn and picked along, in canvas units.
///
/// Bezier detail follows the on-screen length, so a wire stays smooth zoomed
/// in without spending instances zoomed out.
pub fn wire_points(from: Vec2, to: Vec2, routing: Routing, zoom: f32, out: &mut Vec<Vec2>) {
    out.clear();
    match routing {
        Routing::Straight => {
            out.push(from);
            out.push(to);
        }
        Routing::Orthogonal => {
            let mid = (from.x + to.x) * 0.5;
            out.push(from);
            out.push(Vec2::new(mid, from.y));
            out.push(Vec2::new(mid, to.y));
            out.push(to);
        }
        Routing::Bezier => {
            let dx = ((to.x - from.x).abs() * 0.5).max(24.0);
            let (c0, c1) = (Vec2::new(from.x + dx, from.y), Vec2::new(to.x - dx, to.y));
            let len = |a: Vec2, b: Vec2| (b.x - a.x).hypot(b.y - a.y);
            let screen = (len(from, c0) + len(c0, c1) + len(c1, to)) * zoom;
            let n = (screen.max(1.0).sqrt() * 1.2) as usize + 2;
            for i in 0..=n {
                out.push(cubic(from, c0, c1, to, i as f32 / n as f32));
            }
        }
    }
}

/// Distance from `p` to the polyline, in the same units.
pub(crate) fn distance_to(points: &[Vec2], p: Vec2) -> f32 {
    let mut best = f32::MAX;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ba = Vec2::new(b.x - a.x, b.y - a.y);
        let pa = Vec2::new(p.x - a.x, p.y - a.y);
        let len2 = ba.x * ba.x + ba.y * ba.y;
        let t = if len2 > 1e-6 { ((pa.x * ba.x + pa.y * ba.y) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let d = (pa.x - ba.x * t).hypot(pa.y - ba.y * t);
        best = best.min(d);
    }
    best
}
