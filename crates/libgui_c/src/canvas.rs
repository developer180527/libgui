//! The pan/zoom canvas, transforms and animation: what a custom C++ view
//! needs that the widget table cannot express.
//!
//! A CAD sketcher is the case this exists for. [`libgui_open_canvas`] is the
//! difference between writing snap and hit tolerance once in model units and
//! writing a screen-to-world conversion at every call site, because widgets
//! built inside it lay out, hit-test and report their drag deltas in canvas
//! coordinates.

use crate::convert::str_or_empty;
use crate::handle::{with_ui, LibguiUi};
use crate::types::{LibguiRect, LibguiResponse, LibguiVec2};
use libgui::{CanvasState, Id, Transform, Vec2};

/// Where the view is over the canvas, and how far in. Mirrors
/// `libgui::CanvasState`, flattened.
///
/// The app owns this across frames. `visible` is written by
/// [`libgui_open_canvas`] each frame; everything else is read and may be
/// written by the app to drive the view itself (zoom to fit, a zoom box).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiCanvasState {
    /// Canvas origin relative to the widget's top-left, in window pixels.
    pub pan: LibguiVec2,
    pub zoom: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
    /// 1: the wheel zooms (a sketch, a node editor). 0: it scrolls (a
    /// timeline).
    pub wheel_zooms: u8,
    /// Written each frame: the part of the canvas on screen, in canvas
    /// coordinates. Cull against it rather than building what cannot be seen.
    pub visible: LibguiRect,
}

impl Default for LibguiCanvasState {
    fn default() -> Self {
        let d = CanvasState::default();
        Self {
            pan: LibguiVec2 { x: d.pan.x, y: d.pan.y },
            zoom: d.zoom,
            min_zoom: d.min_zoom,
            max_zoom: d.max_zoom,
            wheel_zooms: d.wheel_zooms as u8,
            visible: LibguiRect::default(),
        }
    }
}

impl LibguiCanvasState {
    fn to_core(self) -> CanvasState {
        CanvasState {
            pan: Vec2::new(self.pan.x, self.pan.y),
            zoom: self.zoom,
            min_zoom: self.min_zoom,
            max_zoom: self.max_zoom,
            wheel_zooms: self.wheel_zooms != 0,
            visible: libgui::Rect::new(
                self.visible.x,
                self.visible.y,
                self.visible.w,
                self.visible.h,
            ),
        }
    }

    fn write_back(&mut self, c: CanvasState) {
        self.pan = LibguiVec2 { x: c.pan.x, y: c.pan.y };
        self.zoom = c.zoom;
        self.min_zoom = c.min_zoom;
        self.max_zoom = c.max_zoom;
        self.wheel_zooms = c.wheel_zooms as u8;
        self.visible = LibguiRect { x: c.visible.x, y: c.visible.y, w: c.visible.w, h: c.visible.h };
    }
}

/// A canvas's view for this frame, written by [`libgui_open_canvas`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiCanvasView {
    /// The visible part, in canvas coordinates.
    pub visible: LibguiRect,
    pub zoom: f32,
    /// Canvas coordinates to window coordinates: `window = canvas * zoom + pan`.
    /// Use [`libgui_transform_point`] rather than doing it by hand.
    pub xform_pan: LibguiVec2,
    pub xform_zoom: f32,
}

/// Fill `state` with the defaults, for an app that does not want to know them.
///
/// # Safety
/// `state` must be null or point to a writable `LibguiCanvasState`.
#[no_mangle]
pub unsafe extern "C" fn libgui_canvas_state_default(state: *mut LibguiCanvasState) {
    if !state.is_null() {
        unsafe { *state = LibguiCanvasState::default() };
    }
}

/// Open a pan/zoom canvas: an unbounded coordinate space for a sketch, a node
/// graph, a timeline. Build its contents, then call [`libgui_close_canvas`].
///
/// Widgets built inside it lay out, hit-test and report their rect and drag
/// deltas in **canvas coordinates**, so snapping, hit tolerance and every
/// other piece of app logic is written once and is correct at any zoom. Text
/// is rasterised at the zoomed size rather than scaled up.
///
/// The wheel zooms toward the pointer, the middle button pans, and two fingers
/// pan and pinch around their midpoint — unless the app drives `state` itself.
/// The returned response is the *background's*, so `clicked` on it means the
/// user clicked empty canvas.
///
/// ```c
/// LibguiCanvasState view;
/// libgui_canvas_state_default(&view);      /* once, kept across frames */
///
/// LibguiCanvasView v;
/// LibguiResponse bg = libgui_open_canvas(ui, "sketch", &view, &v);
/// for (size_t i = 0; i < n; i++) {
///     if (!overlaps(ent[i].bounds, v.visible)) continue;   /* cull */
///     draw_entity(ui, &ent[i]);
/// }
/// libgui_close_canvas(ui);
/// if (bg.clicked) deselect_all();
/// ```
///
/// `state` and `out` may be null, though a null `state` means the view cannot
/// be kept and the canvas resets every frame.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_canvas(
    ui: *mut LibguiUi,
    key: *const std::os::raw::c_char,
    state: *mut LibguiCanvasState,
    out: *mut LibguiCanvasView,
) -> LibguiResponse {
    let key = str_or_empty(key, "libgui_open_canvas: key");
    with_ui(ui, LibguiResponse::default(), |u| {
        let mut st = if state.is_null() { LibguiCanvasState::default() } else { *state };
        let mut core = st.to_core();
        let (resp, view) = u.open_canvas(key, &mut core);
        st.write_back(core);
        if !state.is_null() {
            *state = st;
        }
        if !out.is_null() {
            *out = LibguiCanvasView {
                visible: LibguiRect {
                    x: view.visible.x,
                    y: view.visible.y,
                    w: view.visible.w,
                    h: view.visible.h,
                },
                zoom: view.zoom,
                xform_pan: LibguiVec2 { x: view.xform.pan.x, y: view.xform.pan.y },
                xform_zoom: view.xform.zoom,
            };
        }
        LibguiResponse::from(resp)
    })
}

/// Close a canvas opened with [`libgui_open_canvas`].
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_canvas(ui: *mut LibguiUi) {
    with_ui(ui, (), |u| u.close_canvas());
}

/// [`libgui_open_canvas`] without the input handling: draw and interact under
/// a transform you drive yourself. Close it with [`libgui_close_transform`].
///
/// This is the one to use when the view is not a pan and a zoom the library
/// should manage — a sketch plane driven by your own camera, say. `zoom` is
/// clamped away from zero, because a zero-zoom transform has no inverse and
/// every hit test under it would answer the same point.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_transform(
    ui: *mut LibguiUi,
    id: u64,
    pan: LibguiVec2,
    zoom: f32,
) {
    with_ui(ui, (), |u| {
        u.open_transform(Id(id), Transform::new(Vec2::new(pan.x, pan.y), zoom));
    });
}

/// Close a transform opened with [`libgui_open_transform`].
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_transform(ui: *mut LibguiUi) {
    with_ui(ui, (), |u| u.close_transform());
}

/// Canvas coordinates to window coordinates.
#[no_mangle]
pub extern "C" fn libgui_transform_point(pan: LibguiVec2, zoom: f32, p: LibguiVec2) -> LibguiVec2 {
    let t = Transform::new(Vec2::new(pan.x, pan.y), zoom);
    let v = t.point(Vec2::new(p.x, p.y));
    LibguiVec2 { x: v.x, y: v.y }
}

/// Window coordinates to canvas coordinates — a raw pointer position from your
/// own event loop, for instance.
#[no_mangle]
pub extern "C" fn libgui_transform_inv_point(
    pan: LibguiVec2,
    zoom: f32,
    p: LibguiVec2,
) -> LibguiVec2 {
    let t = Transform::new(Vec2::new(pan.x, pan.y), zoom);
    let v = t.inv_point(Vec2::new(p.x, p.y));
    LibguiVec2 { x: v.x, y: v.y }
}

/// Zoom `state` by `factor` about a window position, keeping the canvas point
/// under it still. What a zoom button or a keyboard shortcut calls.
///
/// `origin` is the canvas widget's top-left in window coordinates — the `rect`
/// a previous frame reported for it.
///
/// # Safety
/// `state` must be null or point to a writable `LibguiCanvasState`.
#[no_mangle]
pub unsafe extern "C" fn libgui_canvas_zoom_at(
    state: *mut LibguiCanvasState,
    window_pos: LibguiVec2,
    origin: LibguiVec2,
    factor: f32,
) {
    if state.is_null() {
        return;
    }
    unsafe {
        let mut core = (*state).to_core();
        core.zoom_at(
            Vec2::new(window_pos.x, window_pos.y),
            Vec2::new(origin.x, origin.y),
            factor,
        );
        (*state).write_back(core);
    }
}

/// A retained animation value that eases towards `target`, kept per
/// `(id, slot)` — 256 slots per widget.
///
/// This is how a custom C++ widget moves. Call it every frame with where the
/// value should be and draw with what comes back; the library eases, keeps the
/// value across frames, and asks the host for another frame until it arrives
/// (which is what `repaint_after` in the frame reports).
///
/// The rate is the theme's, and frame-rate independent: the same motion at 60
/// and 144 Hz, and right across a dropped frame.
///
/// These calls mark `id` alive for the frame themselves. libgui drops the
/// retained state of an id it did not see, and in C there is no other way to
/// say an id exists — the alternative failure is an animation that silently
/// resets to its target every frame and never moves at all.
///
/// ```c
/// float hot = libgui_animate_bool(ui, id, 0, resp.hovered);
/// /* ... blend the fill by `hot` ... */
/// ```
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_animate(
    ui: *mut LibguiUi,
    id: u64,
    slot: u8,
    target: f32,
) -> f32 {
    with_ui(ui, 0.0, |u| {
        u.keep_id(Id(id));
        u.animate(Id(id), slot, target)
    })
}

/// [`libgui_animate`] towards 1 when `on`, 0 otherwise: hover, press, open.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_animate_bool(
    ui: *mut LibguiUi,
    id: u64,
    slot: u8,
    on: u8,
) -> f32 {
    with_ui(ui, 0.0, |u| {
        u.keep_id(Id(id));
        u.animate_bool(Id(id), slot, on != 0)
    })
}

/// [`libgui_animate`] with an explicit rate (1/s; higher is snappier).
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_animate_with_speed(
    ui: *mut LibguiUi,
    id: u64,
    slot: u8,
    target: f32,
    speed: f32,
) -> f32 {
    with_ui(ui, 0.0, |u| {
        u.keep_id(Id(id));
        u.animate_with_speed(Id(id), slot, target, speed)
    })
}

/// Jump an animation to `value`; it eases from there towards its next target.
/// For starting a slide from a known place rather than from wherever the last
/// one ended.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_set_anim(ui: *mut LibguiUi, id: u64, slot: u8, value: f32) {
    with_ui(ui, (), |u| {
        u.keep_id(Id(id));
        u.set_anim(Id(id), slot, value);
    });
}

/// Ask for another frame, for a reason the library cannot see: your own
/// simulation is running, a file finished loading, a tool is mid-gesture.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_request_repaint(ui: *mut LibguiUi) {
    with_ui(ui, (), |u| u.request_repaint());
}

/// Keep an id's retained state — animation, scroll offset, text state — alive
/// for a frame in which no widget with that id was built.
///
/// libgui forgets an id it did not see, which is what stops a long session
/// accumulating the state of every widget it ever showed. A row scrolled out
/// of a virtual list, a panel behind a tab: mark it and it is still there when
/// it comes back.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_keep_id(ui: *mut LibguiUi, id: u64) {
    with_ui(ui, (), |u| u.keep_id(Id(id)));
}
