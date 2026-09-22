//! The `Ui` handle, and the rules that keep a C caller from taking the process
//! down with it.

use crate::convert::str_from;
use libgui::{FrameInfo, Theme, Ui, Vec2};
use std::cell::RefCell;
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// An opaque `Ui` plus the state the boundary needs.
pub struct LibguiUi {
    pub(crate) ui: Ui,
    /// A panic crossed this handle. Every call after it does nothing.
    ///
    /// Continuing into half a built frame would corrupt the node tree — the
    /// container stack is already unbalanced — and aborting the host over a UI
    /// bug is worse than either. So the handle stops, says so, and the app
    /// decides what to do.
    pub(crate) poisoned: bool,
}

thread_local! {
    static LAST_ERROR: RefCell<std::ffi::CString> = RefCell::new(std::ffi::CString::default());
}

pub(crate) fn set_error(msg: &str) {
    if let Ok(c) = std::ffi::CString::new(msg) {
        LAST_ERROR.with(|e| *e.borrow_mut() = c);
    }
}

pub(crate) fn last_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        let b = e.borrow();
        if b.as_bytes().is_empty() {
            std::ptr::null()
        } else {
            b.as_ptr()
        }
    })
}

/// Run `body` with the handle, catching anything it throws.
///
/// The one place a panic is allowed to stop: `extern "C"` would abort the
/// process, which is not a library's decision to make about someone else's
/// application.
pub(crate) fn with_ui<R>(ui: *mut LibguiUi, fallback: R, body: impl FnOnce(&mut Ui) -> R) -> R {
    let Some(handle) = (unsafe { ui.as_mut() }) else {
        set_error("null Ui handle");
        return fallback;
    };
    if handle.poisoned {
        return fallback;
    }
    match catch_unwind(AssertUnwindSafe(|| body(&mut handle.ui))) {
        Ok(v) => v,
        Err(e) => {
            let msg = e
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| e.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            set_error(&format!("libgui panicked: {msg}"));
            handle.poisoned = true;
            fallback
        }
    }
}

/// Create a `Ui` with the dark theme and `font_bytes`.
///
/// Returns null if the bytes are not a font this library can read, or if
/// either argument is null; [`crate::libgui_last_error`] says which. The
/// caller owns the handle and frees it with [`libgui_ui_free`].
///
/// # Safety
/// `font_bytes` must point to `font_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn libgui_ui_new(font_bytes: *const u8, font_len: u64) -> *mut LibguiUi {
    if font_bytes.is_null() || font_len == 0 {
        set_error("libgui_ui_new: no font bytes");
        return std::ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(font_bytes, font_len as usize) };
    match Ui::new(Theme::dark(), bytes) {
        Ok(ui) => Box::into_raw(Box::new(LibguiUi { ui, poisoned: false })),
        Err(e) => {
            set_error(&format!("libgui_ui_new: {e}"));
            std::ptr::null_mut()
        }
    }
}

/// Free a handle. Null is fine and does nothing.
///
/// # Safety
/// `ui` must have come from [`libgui_ui_new`] and must not be used again.
#[no_mangle]
pub unsafe extern "C" fn libgui_ui_free(ui: *mut LibguiUi) {
    if !ui.is_null() {
        drop(unsafe { Box::from_raw(ui) });
    }
}

/// Did a panic cross this handle? Everything after one does nothing, so a host
/// that sees this should tear the `Ui` down and build a fresh one rather than
/// carry on.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_ui_poisoned(ui: *mut LibguiUi) -> u8 {
    match unsafe { ui.as_ref() } {
        Some(h) => h.poisoned as u8,
        None => 1,
    }
}

/// Start a frame. `scale` is physical pixels per logical pixel; `dt` is
/// seconds since the last frame.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_begin_frame(ui: *mut LibguiUi, width: f32, height: f32, scale: f32, dt: f32) {
    with_ui(ui, (), |ui| {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(width, height), scale, dt });
    });
}

/// Finish a frame. The draw data stays owned by the library; read it with the
/// `libgui_frame_*` accessors before the next `libgui_begin_frame`.
///
/// # Safety
/// `ui` must be null or a live handle from `libgui_ui_new`. Null is tolerated;
/// a non-null pointer that is not valid is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn libgui_end_frame(ui: *mut LibguiUi) {
    with_ui(ui, (), |ui| {
        let _ = ui.end_frame();
    });
}

/// Set the theme by name: `"dark"`, `"midnight"` or `"light"`. Returns 0 on
/// success, 1 if the name is not one of those.
///
/// # Safety
/// `name` must be a NUL-terminated string or null.
#[no_mangle]
pub unsafe extern "C" fn libgui_set_theme(ui: *mut LibguiUi, name: *const c_char) -> i32 {
    let Some(name) = (unsafe { str_from(name) }) else {
        set_error("libgui_set_theme: name is null or not UTF-8");
        return 1;
    };
    with_ui(ui, 1, |ui| match Theme::preset(name) {
        Some(t) => {
            ui.theme = t;
            0
        }
        None => {
            set_error("libgui_set_theme: unknown preset");
            1
        }
    })
}
