//! Containers, scopes and custom painting: the parts that take a closure in
//! Rust and therefore cannot come from the table.
//!
//! libgui already splits every closure-taking builder into an `open_`/`close_`
//! pair, precisely so a caller without closures can bracket it instead. That
//! was done before this crate existed and is why containers are easy here.
//! Custom painting is the one that genuinely needs a callback.

use crate::handle::{with_ui, LibguiUi};
use crate::types::{LibguiColor, LibguiRect};
use libgui::{Align, Chevron, Color, Frame, Id, Insets, Layout, Painter, Rect, Size, Vec2};
use std::os::raw::c_void;

/// How a size is expressed, flattened from libgui's `Size`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibguiSizeKind {
    /// Exactly `value` logical pixels.
    Fixed = 0,
    /// As large as the contents need.
    Fit = 1,
    /// Share what is left, weighted by `value`.
    Grow = 2,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiSize {
    pub kind: LibguiSizeKind,
    pub value: f32,
}

impl From<LibguiSize> for Size {
    fn from(s: LibguiSize) -> Self {
        match s.kind {
            LibguiSizeKind::Fixed => Size::Fixed(s.value),
            LibguiSizeKind::Fit => Size::Fit,
            LibguiSizeKind::Grow => Size::Grow(s.value),
        }
    }
}

/// A container's layout, flat. The Rust side is a builder, which a C caller
/// cannot chain, so every field is here with a sensible zero.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiLayout {
    /// 0 = row, 1 = column.
    pub axis: u8,
    pub _pad: [u8; 3],
    pub width: LibguiSize,
    pub height: LibguiSize,
    pub pad_left: f32,
    pub pad_right: f32,
    pub pad_top: f32,
    pub pad_bottom: f32,
    pub gap: f32,
    /// 0 = start, 1 = center, 2 = end.
    pub align_main: u8,
    pub align_cross: u8,
    pub _pad2: [u8; 2],
}

fn align(v: u8) -> Align {
    match v {
        1 => Align::Center,
        2 => Align::End,
        _ => Align::Start,
    }
}

impl From<LibguiLayout> for Layout {
    fn from(l: LibguiLayout) -> Self {
        let base = if l.axis == 1 { Layout::column() } else { Layout::row() };
        base.width(l.width.into())
            .height(l.height.into())
            .padding(Insets { left: l.pad_left, right: l.pad_right, top: l.pad_top, bottom: l.pad_bottom })
            .gap(l.gap)
            .align(align(l.align_main), align(l.align_cross))
    }
}

/// A container's background: fill, border and corner radius.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiFrame {
    pub fill: LibguiColor,
    pub border: LibguiColor,
    pub border_width: f32,
    pub radius: f32,
    pub clip: u8,
    pub shadow: u8,
    pub _pad: [u8; 6],
}

fn color(c: LibguiColor) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

impl From<LibguiFrame> for Frame {
    fn from(f: LibguiFrame) -> Self {
        Frame {
            fill: color(f.fill),
            border: color(f.border),
            border_width: f.border_width,
            radius: f.radius,
            clip: f.clip != 0,
            shadow: f.shadow != 0,
        }
    }
}

/// Open a container. Build its children, then call [`libgui_close_container`].
///
/// `id` must be stable across frames and unique among its siblings — use
/// [`libgui_id_from_name`] rather than a counter, or retained state will jump
/// between widgets when the build order changes.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_container(ui: *mut LibguiUi, id: u64, layout: LibguiLayout, frame: LibguiFrame) {
    with_ui(ui, (), |ui| ui.open_container(Id(id), layout.into(), frame.into()));
}

///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_container(ui: *mut LibguiUi) {
    with_ui(ui, (), |ui| ui.close_container());
}

/// How many containers are open. A binding checks this before ending a frame
/// to blame the caller for a missing close, rather than letting libgui's own
/// assertion fire somewhere less useful.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_depth(ui: *mut LibguiUi) -> u64 {
    with_ui(ui, 0, |ui| ui.open_depth() as u64)
}

/// Open a scroll area. Close it with [`libgui_close_scroll_area`].
///
/// # Safety
/// `key` must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_scroll_area(ui: *mut LibguiUi, key: *const std::os::raw::c_char) {
    let key = unsafe { crate::convert::str_or_empty(key, "libgui_open_scroll_area") };
    with_ui(ui, (), |ui| ui.open_scroll_area(key));
}

///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_scroll_area(ui: *mut LibguiUi) {
    with_ui(ui, (), |ui| ui.close_scroll_area());
}

/// Build widgets that cannot be used and look it. Returns what to hand back to
/// [`libgui_close_enabled`].
///
/// Disabling nests one way: opening an enabled scope inside a disabled one
/// does not re-enable.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_enabled(ui: *mut LibguiUi, enabled: u8) -> u8 {
    with_ui(ui, 1, |ui| ui.open_enabled(enabled != 0) as u8)
}

///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_enabled(ui: *mut LibguiUi, was: u8) {
    with_ui(ui, (), |ui| ui.close_enabled(was != 0));
}

/// A stable id from a name. Same value on every platform and every build, so
/// it is safe to persist and to compare across the boundary.
///
/// # Safety
/// `name` must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn libgui_id_from_name(name: *const std::os::raw::c_char) -> u64 {
    let name = unsafe { crate::convert::str_or_empty(name, "libgui_id_from_name") };
    Id::from_name(name).0
}

// ---------------------------------------------------------------------------
// Custom painting
// ---------------------------------------------------------------------------

/// What a C caller draws with. Opaque: the drawing functions take it.
pub struct LibguiPainter {
    inner: *mut c_void,
}

/// A paint callback and the data it needs.
///
/// This is the piece that genuinely needs a function pointer. In Rust,
/// `add_leaf` takes a closure that owns its captures and runs after layout, so
/// the C form needs both halves: `paint` to draw, and `drop_user` to release
/// whatever `user` points at when the frame is over. Leave `drop_user` null if
/// `user` outlives the frame on its own.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LibguiPaintFn {
    pub paint: Option<unsafe extern "C" fn(*mut LibguiPainter, LibguiRect, *mut c_void)>,
    pub drop_user: Option<unsafe extern "C" fn(*mut c_void)>,
    pub user: *mut c_void,
}

/// Holds `user` for the life of the paint closure and frees it after, so a C
/// caller can hand over an allocation and forget about it.
struct UserData(LibguiPaintFn);

impl Drop for UserData {
    fn drop(&mut self) {
        if let Some(f) = self.0.drop_user {
            unsafe { f(self.0.user) };
        }
    }
}

// The pointer is the caller's to keep valid; libgui only moves it into a
// closure that runs on the same thread, in the same frame.
unsafe impl Send for UserData {}

/// A leaf widget the caller draws itself.
///
/// `interactive` makes it hit-testable, so [`libgui_interact`] can report
/// hovers and clicks on `id`.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_add_leaf(
    ui: *mut LibguiUi,
    id: u64,
    layout: LibguiLayout,
    interactive: u8,
    paint: LibguiPaintFn,
) {
    with_ui(ui, (), move |ui| {
        let data = UserData(paint);
        ui.add_leaf(Id(id), layout.into(), Vec2::ZERO, interactive != 0, move |p: &mut Painter, r: Rect| {
            // Held by the closure, so `drop_user` runs when libgui drops it —
            // at the end of the frame, whether or not the widget was visible.
            let d = &data;
            if let Some(f) = d.0.paint {
                let mut handle = LibguiPainter { inner: p as *mut Painter as *mut c_void };
                unsafe { f(&mut handle, r.into(), d.0.user) };
            }
        });
    });
}

/// Resolve hover, press and click for a widget the caller drew.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_interact(ui: *mut LibguiUi, id: u64) -> crate::types::LibguiResponse {
    with_ui(ui, Default::default(), |ui| ui.interact(Id(id)).into())
}

fn painter<'a>(p: *mut LibguiPainter) -> Option<&'a mut Painter<'a>> {
    let p = unsafe { p.as_mut() }?;
    Some(unsafe { &mut *(p.inner as *mut Painter) })
}

/// Fill a rounded rectangle.
///
/// # Safety
/// `p` must be null or the painter handed to a `LibguiPaintFn` callback, used
/// only for the duration of that call.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_rect(p: *mut LibguiPainter, r: LibguiRect, fill: LibguiColor, radius: f32) {
    if let Some(p) = painter(p) {
        p.rect(Rect::new(r.x, r.y, r.w, r.h), color(fill), radius);
    }
}

/// Fill a rounded rectangle and stroke its border.
///
/// # Safety
/// `p` must be null or the painter handed to a `LibguiPaintFn` callback, used
/// only for the duration of that call.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_rect_bordered(
    p: *mut LibguiPainter,
    r: LibguiRect,
    fill: LibguiColor,
    radius: f32,
    border_width: f32,
    border: LibguiColor,
) {
    if let Some(p) = painter(p) {
        p.rect_bordered(Rect::new(r.x, r.y, r.w, r.h), color(fill), radius, border_width, color(border));
    }
}

/// A line between two points.
///
/// # Safety
/// `p` must be null or the painter handed to a `LibguiPaintFn` callback, used
/// only for the duration of that call.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_line(
    p: *mut LibguiPainter,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    width: f32,
    c: LibguiColor,
) {
    if let Some(p) = painter(p) {
        p.line(Vec2::new(x0, y0), Vec2::new(x1, y1), width, color(c));
    }
}

/// Draw one of your own textures into `r`, with `radius` rounding its
/// corners. `texture` is the index your renderer registered; it comes back in
/// `LibguiBatch::texture_index` with `texture_kind` 1.
///
/// This is the painter form, for an icon or a thumbnail inside a custom leaf.
/// For the 3D view itself, `libgui_viewport` is the widget.
///
/// # Safety
/// `p` must be the painter handed to a paint callback, or null.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_image(p: *mut LibguiPainter, r: LibguiRect, texture: u64, radius: f32) {
    if let Some(p) = painter(p) {
        p.image(Rect::new(r.x, r.y, r.w, r.h), libgui::TextureId::User(texture), radius);
    }
}

/// Draw one of your own textures into `r`, taking only the part of it between
/// `(u0, v0)` and `(u1, v1)` — one sprite out of a sheet — and tinting it by
/// `tint`. Pass white for no tint.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_image_uv(
    p: *mut LibguiPainter,
    r: LibguiRect,
    texture: u64,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    radius: f32,
    tint: LibguiColor,
) {
    if let Some(p) = painter(p) {
        let tex = libgui::TextureId::User(texture);
        p.image_tinted(Rect::new(r.x, r.y, r.w, r.h), tex, [u0, v0, u1, v1], radius, color(tint));
    }
}

/// Text at the left of `r`, vertically centred.
///
/// # Safety
/// `p` must be null or the painter handed to a paint callback, and `text` a
/// NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_text_left(
    p: *mut LibguiPainter,
    r: LibguiRect,
    size: f32,
    c: LibguiColor,
    text: *const std::os::raw::c_char,
) {
    let text = unsafe { crate::convert::str_or_empty(text, "libgui_painter_text_left") };
    if let Some(p) = painter(p) {
        // `str` implements `PaintText`, so the caller's bytes are used
        // directly; nothing is copied into the frame arena and nothing is
        // allocated to cross back.
        p.text_left(Rect::new(r.x, r.y, r.w, r.h), size, color(c), text);
    }
}

// ---------------------------------------------------------------------------
// The rest of the painter
// ---------------------------------------------------------------------------
//
// A custom widget drawn from C had a third of the palette a Rust one has,
// which made "you can build your own widgets" true in principle and thin in
// practice. These are the other thirteen. `measure` matters most — without it
// a caller cannot size its own text, so it cannot lay anything out.

/// Which way a chevron points: 0 up, 1 down, 2 left, 3 right.
fn chevron_of(d: u32) -> Chevron {
    match d {
        1 => Chevron::Down,
        2 => Chevron::Left,
        _ if d == 3 => Chevron::Right,
        _ => Chevron::Up,
    }
}

/// How large `text` would be at `size`, in logical pixels.
///
/// The one call a custom widget cannot do without: everything else draws, this
/// is how you decide *where*. Writes the size into `out_w`/`out_h`.
///
/// # Safety
/// `p` must be the painter handed to a paint callback; `text` NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_measure(
    p: *mut LibguiPainter,
    size: f32,
    text: *const std::os::raw::c_char,
    out_w: *mut f32,
    out_h: *mut f32,
) {
    let text = unsafe { crate::convert::str_or_empty(text, "libgui_painter_measure") };
    let v = match painter(p) {
        Some(p) => p.measure(size, text),
        None => Vec2::ZERO,
    };
    if let Some(s) = unsafe { out_w.as_mut() } {
        *s = v.x;
    }
    if let Some(s) = unsafe { out_h.as_mut() } {
        *s = v.y;
    }
}

/// A rectangle exactly `px` physical pixels wide at `x`, however the display
/// is scaled.
///
/// A 1.0-wide rect on a 1.5× display lands on a pixel and a half and renders
/// as a grey smear. This is what keeps a rule, a grid line or a dimension
/// witness line crisp — which a CAD drawing is mostly made of.
///
/// # Safety
/// As [`libgui_painter_measure`]; `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_hairline(
    p: *mut LibguiPainter,
    x: f32,
    y: f32,
    px: f32,
    height: f32,
    out: *mut LibguiRect,
) {
    let r = match painter(p) {
        Some(p) => p.hairline(x, y, px, height),
        None => Rect::default(),
    };
    if let Some(s) = unsafe { out.as_mut() } {
        *s = r.into();
    }
}

/// `r` snapped to whole physical pixels, so its edges are hard.
///
/// # Safety
/// As [`libgui_painter_hairline`].
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_snap_rect(p: *mut LibguiPainter, r: LibguiRect, out: *mut LibguiRect) {
    let v = match painter(p) {
        Some(p) => p.snap_rect(Rect::new(r.x, r.y, r.w, r.h)),
        None => Rect::default(),
    };
    if let Some(s) = unsafe { out.as_mut() } {
        *s = v.into();
    }
}

/// A soft blurred rounded rect: a drop shadow or a glow.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_shadow(
    p: *mut LibguiPainter,
    r: LibguiRect,
    radius: f32,
    blur: f32,
    c: LibguiColor,
) {
    if let Some(p) = painter(p) {
        p.shadow(Rect::new(r.x, r.y, r.w, r.h), radius, blur, color(c));
    }
}

/// A texture multiplied by a tint, with explicit uv coordinates. For an icon
/// sheet recoloured per state.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn libgui_painter_image_tinted(
    p: *mut LibguiPainter,
    r: LibguiRect,
    texture_index: u64,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    radius: f32,
    tint: LibguiColor,
) {
    if let Some(p) = painter(p) {
        p.image_tinted(
            Rect::new(r.x, r.y, r.w, r.h),
            libgui::TextureId::User(texture_index),
            [u0, v0, u1, v1],
            radius,
            color(tint),
        );
    }
}

/// A connected run of line segments. `points` is `count` pairs of floats.
///
/// # Safety
/// `points` must hold `count * 2` readable floats.
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_polyline(
    p: *mut LibguiPainter,
    points: *const f32,
    count: u64,
    width: f32,
    c: LibguiColor,
) {
    if points.is_null() || count == 0 {
        return;
    }
    // Read in place rather than copied into a `Vec`. This is a paint callback
    // — it runs for every polyline of every frame — and a CAD drawing is
    // mostly polylines, so allocating per call is the wrong shape. `Vec2` is
    // `#[repr(C)]`, two `f32`s with no padding, which the assertions below
    // pin; every bit pattern is a valid `f32`, so the caller's array *is* a
    // slice of points.
    const _: () = assert!(std::mem::size_of::<Vec2>() == 2 * std::mem::size_of::<f32>());
    const _: () = assert!(std::mem::align_of::<Vec2>() == std::mem::align_of::<f32>());
    let pts = unsafe { std::slice::from_raw_parts(points as *const Vec2, count as usize) };
    if let Some(p) = painter(p) {
        p.polyline(pts, width, color(c));
    }
}

/// A cubic Bézier from `p0` to `p1` with controls `c0` and `c1`. For a curve
/// in a node graph, a spline in a sketch, an easing preview.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn libgui_painter_bezier(
    p: *mut LibguiPainter,
    x0: f32,
    y0: f32,
    cx0: f32,
    cy0: f32,
    cx1: f32,
    cy1: f32,
    x1: f32,
    y1: f32,
    width: f32,
    c: LibguiColor,
) {
    if let Some(p) = painter(p) {
        p.bezier(
            Vec2::new(x0, y0),
            Vec2::new(cx0, cy0),
            Vec2::new(cx1, cy1),
            Vec2::new(x1, y1),
            width,
            color(c),
        );
    }
}

/// A link that leaves its start sideways and arrives sideways, the way a node
/// graph draws a connection.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_wire(
    p: *mut LibguiPainter,
    from_x: f32,
    from_y: f32,
    to_x: f32,
    to_y: f32,
    width: f32,
    c: LibguiColor,
) {
    if let Some(p) = painter(p) {
        p.wire(Vec2::new(from_x, from_y), Vec2::new(to_x, to_y), width, color(c));
    }
}

/// A disclosure arrow, drawn as two strokes rather than a glyph so it stays
/// crisp and needs no font. `dir` is 0 up, 1 down, 2 left, 3 right.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_chevron(
    p: *mut LibguiPainter,
    r: LibguiRect,
    size: f32,
    dir: u32,
    c: LibguiColor,
) {
    if let Some(p) = painter(p) {
        p.chevron(Rect::new(r.x, r.y, r.w, r.h), size, chevron_of(dir), color(c));
    }
}

/// Text with its top-left at a point.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_text(
    p: *mut LibguiPainter,
    x: f32,
    y: f32,
    size: f32,
    c: LibguiColor,
    text: *const std::os::raw::c_char,
) {
    let text = unsafe { crate::convert::str_or_empty(text, "libgui_painter_text") };
    if let Some(p) = painter(p) {
        p.text(Vec2::new(x, y), size, color(c), text);
    }
}

/// Text against the right edge of `r`, vertically centred. What a numeric
/// column wants.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_text_right(
    p: *mut LibguiPainter,
    r: LibguiRect,
    size: f32,
    c: LibguiColor,
    text: *const std::os::raw::c_char,
) {
    let text = unsafe { crate::convert::str_or_empty(text, "libgui_painter_text_right") };
    if let Some(p) = painter(p) {
        p.text_right(Rect::new(r.x, r.y, r.w, r.h), size, color(c), text);
    }
}

/// Text centred in `r`, both ways.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_text_centered(
    p: *mut LibguiPainter,
    r: LibguiRect,
    size: f32,
    c: LibguiColor,
    text: *const std::os::raw::c_char,
) {
    let text = unsafe { crate::convert::str_or_empty(text, "libgui_painter_text_centered") };
    if let Some(p) = painter(p) {
        p.text_centered(Rect::new(r.x, r.y, r.w, r.h), size, color(c), text);
    }
}

/// Text wrapped to `r`'s width. `align` is 0 left, 1 centre, 2 right.
///
/// # Safety
/// As [`libgui_painter_measure`].
#[no_mangle]
pub unsafe extern "C" fn libgui_painter_text_wrapped(
    p: *mut LibguiPainter,
    r: LibguiRect,
    size: f32,
    c: LibguiColor,
    align: u32,
    text: *const std::os::raw::c_char,
) {
    let text = unsafe { crate::convert::str_or_empty(text, "libgui_painter_text_wrapped") };
    let a = match align {
        1 => Align::Center,
        2 => Align::End,
        _ => Align::Start,
    };
    if let Some(p) = painter(p) {
        p.text_wrapped(Rect::new(r.x, r.y, r.w, r.h), size, color(c), a, text);
    }
}

// ---------------------------------------------------------------------------
// Subtree caching
// ---------------------------------------------------------------------------

/// Replay a subtree's pixels instead of building it again.
///
/// Returns **1 when the subtree has to be built**: build it, then call
/// [`libgui_close_cached`]. Returns 0 when the recording was replayed — build
/// nothing and close nothing.
///
/// `deps` is a number you choose: a revision counter, a hash of the data the
/// subtree draws, anything that changes when the pixels would. Rebuild happens
/// when it changes, and only then — profiling puts paint at about ninety per
/// cent of a frame, so this is where a busy panel's cost actually is.
///
/// **A section that updates at its own rate** is this and nothing else: put a
/// coarse tick in `deps` and it rebuilds at that rate and replays in between.
///
/// ```c
/// uint64_t tick = (uint64_t)(now * 10.0);      /* ten times a second */
/// if (libgui_open_cached(ui, "telemetry", tick)) {
///     build_telemetry_panel(ui);
///     libgui_close_cached(ui);
/// }
/// ```
///
/// It refuses to replay when that would be wrong, and you do not manage any of
/// it: the pointer is over it, focus is inside it, it is still animating, the
/// DPI scale or canvas transform changed, the glyph atlas was repacked, or it
/// moved while a pointer was inside it — a different widget is under that
/// pointer now.
///
/// A replay survives the subtree *moving* but not *resizing*; a resize rebuilds.
///
/// # Safety
/// `ui` must be null or live; `key` null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_cached(ui: *mut LibguiUi, key: *const std::os::raw::c_char, deps: u64) -> u8 {
    let key = unsafe { crate::convert::str_or_empty(key, "libgui_open_cached") };
    with_ui(ui, 0, |u| u.open_cached(key, deps) as u8)
}

/// Close the subtree [`libgui_open_cached`] opened. Only call this when it
/// returned 1.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_cached(ui: *mut LibguiUi) {
    with_ui(ui, (), |u| u.close_cached());
}
