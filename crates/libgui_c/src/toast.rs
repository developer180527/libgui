//! Notifications that come and go, and the timed wake-up they sleep on.

use crate::convert::str_or_empty;
use crate::handle::{with_ui, LibguiUi};
use libgui::{Toast, ToastCorner, ToastId, ToastKind, ToastOptions, ToastResponse};
use std::os::raw::c_char;

/// `LIBGUI_TOAST_INFO` .. `LIBGUI_TOAST_ERROR`; anything else is info.
fn kind(k: i32) -> ToastKind {
    match k {
        1 => ToastKind::Success,
        2 => ToastKind::Warning,
        3 => ToastKind::Error,
        _ => ToastKind::Info,
    }
}

/// Mirrors [`libgui::ToastOptions`]. Start from
/// [`libgui_toast_options_default`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiToastOptions {
    /// `LIBGUI_TOAST_BOTTOM_RIGHT` (0), bottom-left, top-right, top-left.
    pub corner: i32,
    /// At most this many at once; the rest wait their turn.
    pub max_visible: u32,
    pub width: f32,
    /// Distance from the window's edges.
    pub margin: f32,
}

impl From<LibguiToastOptions> for ToastOptions {
    fn from(o: LibguiToastOptions) -> Self {
        let corner = match o.corner {
            1 => ToastCorner::BottomLeft,
            2 => ToastCorner::TopRight,
            3 => ToastCorner::TopLeft,
            _ => ToastCorner::BottomRight,
        };
        Self { corner, max_visible: o.max_visible as usize, width: o.width, margin: o.margin }
    }
}

/// # Safety
/// `out` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_toast_options_default(out: *mut LibguiToastOptions) {
    if let Some(o) = unsafe { out.as_mut() } {
        let d = ToastOptions::default();
        *o = LibguiToastOptions { corner: 0, max_visible: d.max_visible as u32, width: d.width, margin: d.margin };
    }
}

/// Mirrors [`libgui::ToastResponse`]; 0 is "none".
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiToastResponse {
    /// This notification's action was pressed. It is dismissed too.
    pub action: u64,
    /// This notification was closed by hand.
    pub closed: u64,
}

impl From<ToastResponse> for LibguiToastResponse {
    fn from(r: ToastResponse) -> Self {
        Self { action: r.action.map_or(0, |t| t.0), closed: r.closed.map_or(0, |t| t.0) }
    }
}

/// Queue a notification; returns its id (never 0). Callable at any time,
/// between frames too.
///
/// `duration` is seconds on screen: negative for the kind's own (four seconds;
/// an error stays until closed), 0 to stay until closed. `action` is a button
/// label, or null for none.
///
/// # Safety
/// `ui` null or live; `message` a string; `action` null or a string.
#[no_mangle]
pub unsafe extern "C" fn libgui_toast(
    ui: *mut LibguiUi,
    kind_: i32,
    message: *const c_char,
    duration: f32,
    action: *const c_char,
) -> u64 {
    let message = unsafe { str_or_empty(message, "libgui_toast") }.to_owned();
    let action = (!action.is_null()).then(|| unsafe { str_or_empty(action, "libgui_toast") }.to_owned());
    let mut t = Toast { kind: kind(kind_), ..Toast::new(message) };
    if kind(kind_) == ToastKind::Error {
        t = t.sticky();
    }
    if duration == 0.0 {
        t = t.sticky();
    } else if duration > 0.0 {
        t = t.duration(duration);
    }
    t.action = action;
    with_ui(ui, 0, |u| u.toast(t).0)
}

/// Take a notification away; it slides out on the next frame.
///
/// # Safety
/// `ui` null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dismiss_toast(ui: *mut LibguiUi, id: u64) {
    with_ui(ui, (), |u| u.dismiss_toast(ToastId(id)));
}

/// How many notifications are showing or waiting.
///
/// # Safety
/// `ui` null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_toast_count(ui: *mut LibguiUi) -> u64 {
    with_ui(ui, 0, |u| u.toast_count() as u64)
}

/// Draw the notifications: once a frame, last. `opts` may be null.
///
/// # Safety
/// `ui` null or live; `opts` null or valid.
#[no_mangle]
pub unsafe extern "C" fn libgui_show_toasts(ui: *mut LibguiUi, opts: *const LibguiToastOptions) -> LibguiToastResponse {
    let o = unsafe { opts.as_ref() }.map(|o| ToastOptions::from(*o)).unwrap_or_default();
    with_ui(ui, LibguiToastResponse::default(), |u| u.show_toasts_with(o).into())
}

/// Ask for a frame in `seconds`, not now: the host may sleep until then.
/// Arrives as `repaint_after`, the soonest of every request this frame.
///
/// # Safety
/// `ui` null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_request_repaint_in(ui: *mut LibguiUi, seconds: f32) {
    with_ui(ui, (), |u| u.request_repaint_in(seconds));
}
