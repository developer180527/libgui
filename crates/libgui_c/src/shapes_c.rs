//! Filled polygons and meshes, drawn as triangles.

use crate::containers::LibguiPainter;
use crate::types::LibguiColor;
use libgui::{Color, Vec2};

fn color(c: LibguiColor) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

/// `count` points at `xy` (x, y, x, y, …), read in place — `Vec2` is two
/// `f32`s with no padding, as `libgui_painter_polyline` relies on too.
unsafe fn points<'a>(xy: *const f32, count: u64) -> &'a [Vec2] {
    const _: () = assert!(std::mem::size_of::<Vec2>() == 2 * std::mem::size_of::<f32>());
    if xy.is_null() || count == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(xy as *const Vec2, count as usize) }
}

/// A filled polygon, `count` points at `xy` (x, y pairs), either winding,
/// concave or not. Anti-aliased edges; no seams inside, so translucent fills
/// are clean. Drawn as triangles every call, nothing cached: for shapes that
/// change every frame.
///
/// # Safety
/// `p` null or the painter handed to a paint callback; `xy` null or `2 *
/// count` floats.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_fill_polygon(p: *mut LibguiPainter, xy: *const f32, count: u64, c: LibguiColor) {
    let pts = unsafe { points(xy, count) };
    if let Some(p) = crate::containers::painter(p) {
        p.fill_polygon(pts, color(c));
    }
}

/// [`libgui_painter_fill_polygon`] with `hole_count` holes cut out: hole `i`
/// is `hole_counts[i]` points at `holes[i]`.
///
/// # Safety
/// As [`libgui_painter_fill_polygon`]; `holes` and `hole_counts` null or
/// `hole_count` entries, each hole null or `2 * hole_counts[i]` floats.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_fill_polygon_with_holes(
    p: *mut LibguiPainter,
    xy: *const f32,
    count: u64,
    holes: *const *const f32,
    hole_counts: *const u64,
    hole_count: u64,
    c: LibguiColor,
) {
    let outline = unsafe { points(xy, count) };
    let mut list: Vec<&[Vec2]> = Vec::new();
    if !holes.is_null() && !hole_counts.is_null() {
        let hs = unsafe { std::slice::from_raw_parts(holes, hole_count as usize) };
        let ns = unsafe { std::slice::from_raw_parts(hole_counts, hole_count as usize) };
        list.extend(hs.iter().zip(ns).map(|(&h, &n)| unsafe { points(h, n) }));
    }
    if let Some(p) = crate::containers::painter(p) {
        p.fill_polygon_with_holes(outline, &list, color(c));
    }
}

/// Triangles you made: `index_count / 3` of them, indices into `count`
/// points at `xy`. Edges used by one triangle are anti-aliased; shared edges
/// meet exactly. Indices out of range drop their triangle.
///
/// # Safety
/// As [`libgui_painter_fill_polygon`]; `indices` null or `index_count`
/// entries.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_fill_mesh(
    p: *mut LibguiPainter,
    xy: *const f32,
    count: u64,
    indices: *const u32,
    index_count: u64,
    c: LibguiColor,
) {
    let pts = unsafe { points(xy, count) };
    let idx: &[u32] = if indices.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(indices, index_count as usize) } };
    if let Some(p) = crate::containers::painter(p) {
        p.fill_mesh(pts, idx, color(c));
    }
}
