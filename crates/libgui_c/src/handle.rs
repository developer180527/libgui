//! The `Ui` handle, and the rules that keep a C caller from taking the process
//! down with it.

use crate::convert::str_from;
use libgui::{FrameInfo, Theme, Ui, Vec2};
use std::cell::RefCell;
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// An opaque `Ui` plus the state the boundary needs.
pub struct LibguiUi {
    /// What every call derives its borrow from.
    ///
    /// Normally this points into `owner`. During a callback — a dock panel
    /// drawing itself — it points at the `&mut Ui` libgui handed us, so nested
    /// calls **reborrow through that one live borrow** instead of starting a
    /// second one. Two `&mut` to the same `Ui` would be undefined behaviour;
    /// this is the difference between a callback that works and one that works
    /// until the optimiser notices.
    pub(crate) ui: *mut Ui,
    /// This handle allocated the `Ui` and frees it.
    ///
    /// A `Box<Ui>` beside the pointer would be the obvious way to own it, and
    /// it is wrong: the pointer is derived from the box, the box is then moved
    /// into this struct, and moving it invalidates everything derived from it.
    /// Miri catches that; a test cannot, because the address is the same
    /// either way. So the raw pointer *is* the owner.
    owns_ui: bool,
    /// How deep inside libgui callbacks we are. Frame-level operations are
    /// refused above zero: freeing the handle or ending the frame from inside
    /// a panel would pull the ground out from under the walk in progress.
    pub(crate) depth: u32,
    /// Captured at `end_frame`, because `FrameOutput` borrows the `Ui` and
    /// cannot itself cross. Valid until the next `begin_frame`.
    pub(crate) frame: crate::frame::FrameData,
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
    // The pointer is copied out and the borrow of the handle ends here, so
    // nothing holds a `&mut LibguiUi` across `body` — which could re-enter
    // through this same handle.
    let uip = {
        let Some(handle) = (unsafe { ui.as_mut() }) else {
            set_error("null Ui handle");
            return fallback;
        };
        if handle.poisoned {
            return fallback;
        }
        handle.ui
    };
    if uip.is_null() {
        set_error("Ui handle has no Ui");
        return fallback;
    }
    match catch_unwind(AssertUnwindSafe(|| body(unsafe { &mut *uip }))) {
        Ok(v) => v,
        Err(e) => {
            let msg = e
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| e.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            set_error(&format!("libgui panicked: {msg}"));
            if let Some(h) = unsafe { ui.as_mut() } {
                h.poisoned = true;
            }
            fallback
        }
    }
}

/// Is this handle inside a libgui callback? Frame-level calls are refused
/// there, because unwinding the frame from inside a panel would corrupt the
/// walk that is drawing it.
pub(crate) fn inside_callback(ui: *mut LibguiUi, what: &str) -> bool {
    match unsafe { ui.as_ref() } {
        Some(h) if h.depth > 0 => {
            set_error(&format!("{what}: not allowed inside a panel callback"));
            true
        }
        _ => false,
    }
}

/// Point a handle at a borrowed `Ui` for the duration of a callback, then put
/// it back. See the note on [`LibguiUi::ui`].
pub(crate) struct Borrowed {
    handle: *mut LibguiUi,
    saved: *mut Ui,
}

impl Borrowed {
    pub(crate) fn new(handle: *mut LibguiUi, ui: &mut Ui) -> Self {
        let saved = unsafe { (*handle).ui };
        unsafe {
            (*handle).ui = ui as *mut Ui;
            (*handle).depth += 1;
        }
        Self { handle, saved }
    }
}

impl Drop for Borrowed {
    fn drop(&mut self) {
        unsafe {
            (*self.handle).ui = self.saved;
            (*self.handle).depth -= 1;
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
        Ok(ui) => into_handle(ui),
        Err(e) => {
            set_error(&format!("libgui_ui_new: {e}"));
            std::ptr::null_mut()
        }
    }
}

/// Create a second `Ui` drawing from the same font system as `other`: the
/// same faces, the same shaping caches, and **one glyph atlas**.
///
/// This is what a docked application wants for a torn-off window. Without it
/// each window rasterises every glyph again and you upload another copy of the
/// same image — for a CJK interface, thousands of glyphs and megabytes of
/// texture per window. With it there is one atlas: upload it once and let
/// every window's draw calls sample it.
///
/// `libgui_frame_atlas` then reports the same pointer and the same version for
/// every window sharing it, so the "has it changed?" check a host already does
/// is all it needs.
///
/// The new `Ui` has its own theme, input, focus and widget state. Only the
/// font system is shared, and it lives until the last handle sharing it is
/// freed — the order they are freed in does not matter.
///
/// Returns null if `other` is null or poisoned.
///
/// # Safety
/// `other` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_ui_new_sharing_fonts(other: *mut LibguiUi) -> *mut LibguiUi {
    let Some(handle) = (unsafe { other.as_mut() }) else {
        set_error("libgui_ui_new_sharing_fonts: null Ui handle");
        return std::ptr::null_mut();
    };
    if handle.poisoned || handle.ui.is_null() {
        set_error("libgui_ui_new_sharing_fonts: the Ui is poisoned");
        return std::ptr::null_mut();
    }
    // A shared borrow is enough, and nothing here re-enters.
    let source = unsafe { &*handle.ui };
    into_handle(Ui::sharing_fonts(source.theme.clone(), source))
}

/// Wrap a `Ui` in a handle the caller owns.
///
/// The raw pointer *is* the owner: a `Box<Ui>` beside it would be derived from
/// a box that then moves into this struct, which invalidates the pointer. Miri
/// caught that; a test cannot, because the address is the same either way.
pub(crate) fn into_handle(ui: Ui) -> *mut LibguiUi {
    Box::into_raw(Box::new(LibguiUi {
        ui: Box::into_raw(Box::new(ui)),
        owns_ui: true,
        depth: 0,
        frame: Default::default(),
        poisoned: false,
    }))
}

/// Free a handle. Null is fine and does nothing.
///
/// # Safety
/// `ui` must have come from [`libgui_ui_new`] and must not be used again.
#[no_mangle]
pub unsafe extern "C" fn libgui_ui_free(ui: *mut LibguiUi) {
    if ui.is_null() || inside_callback(ui, "libgui_ui_free") {
        return;
    }
    let handle = unsafe { Box::from_raw(ui) };
    if handle.owns_ui && !handle.ui.is_null() {
        drop(unsafe { Box::from_raw(handle.ui) });
    }
    drop(handle);
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
    if inside_callback(ui, "libgui_begin_frame") {
        return;
    }
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
    if inside_callback(ui, "libgui_end_frame") {
        return;
    }
    let Some(handle) = (unsafe { ui.as_mut() }) else {
        set_error("null Ui handle");
        return;
    };
    if handle.poisoned {
        return;
    }
    // A split borrow, not `with_ui`: `end_frame` borrows the `Ui` while the
    // capture writes into a sibling field of the same handle.
    let uip = handle.ui;
    let LibguiUi { frame, poisoned, .. } = handle;
    if uip.is_null() {
        return;
    }
    // A container the caller never closed. libgui's own check for this is a
    // `debug_assert`, which is nothing in the release build a shipping
    // application uses — and the frame that comes out is quietly wrong rather
    // than absent. Half a built frame is not worth continuing into, so it is
    // reported here instead, where the caller can be named.
    let open = unsafe { &*uip }.open_depth();
    if open > 0 {
        set_error(&format!(
            "libgui_end_frame: {open} container(s) still open — every open_* needs its close_*"
        ));
        *poisoned = true;
        return;
    }
    if catch_unwind(AssertUnwindSafe(|| {
        let out = unsafe { &mut *uip }.end_frame();
        crate::frame::capture(&out, frame);
    }))
    .is_err()
    {
        set_error("libgui panicked in end_frame");
        *poisoned = true;
    }
}

/// Read this frame's captured output. Nothing is computed here — the data was
/// taken at `end_frame` and is valid until the next `begin_frame`.
pub(crate) fn with_frame<R>(ui: *mut LibguiUi, fallback: R, body: impl FnOnce(&crate::frame::FrameData) -> R) -> R {
    match unsafe { ui.as_ref() } {
        Some(h) if !h.poisoned => body(&h.frame),
        _ => fallback,
    }
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
