//! The C API driven the way C drives it: raw pointers, NUL-terminated strings,
//! out-parameters, and a paint callback.
//!
//! In Rust so it runs in CI on every platform without a C toolchain; the C
//! smoke test compiles the header against the real static library and is the
//! other half of this.

// Calling a C ABI is unsafe by nature; these tests accept that the same way a
// C caller does, and wrap whole bodies rather than every line.
#![allow(unused_unsafe)]

use libgui_c::*;
use std::ffi::CString;
use std::os::raw::c_void;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

unsafe fn ui() -> *mut LibguiUi {
    let p = unsafe { libgui_ui_new(FONT.as_ptr(), FONT.len() as u64) };
    assert!(!p.is_null(), "libgui_ui_new returned null");
    p
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

unsafe fn frame(ui: *mut LibguiUi, body: impl FnOnce()) {
    unsafe {
        libgui_begin_frame(ui, 400.0, 300.0, 1.0, 1.0 / 60.0);
        body();
        libgui_end_frame(ui);
    }
}

#[test]
fn the_abi_version_is_reported() {
unsafe {    assert_eq!(libgui_abi_version(), LIBGUI_ABI_VERSION);}
}

#[test]
fn a_ui_is_created_used_and_freed() {
unsafe {    let u = ui();
    assert_eq!(libgui_ui_poisoned(u), 0);
    let label = c("Bodies");
    frame(u, || libgui_label(u, label.as_ptr()));
    libgui_ui_free(u);}
}

/// Bad font bytes are an error, not a crash, and say so.
#[test]
fn a_bad_font_returns_null_and_an_error() {
unsafe {    let junk = [0u8; 8];
    let p = libgui_ui_new(junk.as_ptr(), junk.len() as u64);
    assert!(p.is_null());
    assert!(!libgui_last_error().is_null(), "no error was recorded");
    assert!(libgui_ui_new(std::ptr::null(), 0).is_null());}
}

/// Out-parameters are read and written, which is how C gets a value back.
#[test]
fn a_checkbox_reads_and_writes_the_callers_byte() {
unsafe {    let u = ui();
    let label = c("Visible");
    let mut on: u8 = 1;
    frame(u, || {
        let r = libgui_checkbox(u, label.as_ptr(), &mut on as *mut u8);
        assert_eq!(r.clicked, 0);
    });
    assert_eq!(on, 1, "an untouched checkbox changed the caller's value");

    let mut v: f32 = 0.25;
    let sl = c("Radius");
    frame(u, || {
        libgui_slider(u, sl.as_ptr(), &mut v as *mut f32, 0.0, 1.0);
    });
    assert_eq!(v, 0.25);
    libgui_ui_free(u);}
}

/// **Nothing crashes on a null.** A C caller will pass one eventually, and a
/// UI library that segfaults over a label is not one anybody can ship.
#[test]
fn nulls_are_tolerated_everywhere() {
unsafe {    let u = ui();
    frame(u, || {
        libgui_label(u, std::ptr::null());
        libgui_button(u, std::ptr::null());
        libgui_checkbox(u, std::ptr::null(), std::ptr::null_mut());
        libgui_slider(u, std::ptr::null(), std::ptr::null_mut(), 0.0, 1.0);
    });
    assert_eq!(libgui_ui_poisoned(u), 0, "a null argument poisoned the handle");

    // And a null handle is inert rather than fatal.
    let label = c("x");
    libgui_label(std::ptr::null_mut(), label.as_ptr());
    assert_eq!(libgui_ui_poisoned(std::ptr::null_mut()), 1);
    libgui_ui_free(u);
    libgui_ui_free(std::ptr::null_mut());}
}

/// Invalid UTF-8 draws an empty label and records why, rather than aborting.
#[test]
fn invalid_utf8_is_reported_not_fatal() {
unsafe {    let u = ui();
    let bad = [0xffu8, 0xfe, 0x00];
    frame(u, || {
        libgui_label(u, bad.as_ptr() as *const std::os::raw::c_char);
    });
    assert_eq!(libgui_ui_poisoned(u), 0);
    assert!(!libgui_last_error().is_null());
    libgui_ui_free(u);}
}

/// Containers bracket, because the Rust side already has open/close pairs.
#[test]
fn containers_bracket_and_the_depth_is_visible() {
unsafe {    let u = ui();
    let layout = LibguiLayout {
        axis: 1,
        _pad: [0; 3],
        width: LibguiSize { kind: LibguiSizeKind::Grow, value: 1.0 },
        height: LibguiSize { kind: LibguiSizeKind::Fit, value: 0.0 },
        pad_left: 8.0,
        pad_right: 8.0,
        pad_top: 8.0,
        pad_bottom: 8.0,
        gap: 4.0,
        align_main: 0,
        align_cross: 0,
        _pad2: [0; 2],
    };
    let frame_ = LibguiFrame {
        fill: LibguiColor { r: 0.1, g: 0.1, b: 0.1, a: 1.0 },
        border: LibguiColor::default(),
        border_width: 0.0,
        radius: 4.0,
        clip: 1,
        shadow: 0,
        _pad: [0; 6],
    };
    let id = libgui_id_from_name(c("panel").as_ptr());
    let label = c("Inspector");
    frame(u, || {
        assert_eq!(libgui_open_depth(u), 0);
        libgui_open_container(u, id, layout, frame_);
        assert_eq!(libgui_open_depth(u), 1);
        libgui_label(u, label.as_ptr());
        libgui_close_container(u);
        assert_eq!(libgui_open_depth(u), 0);
    });
    assert_eq!(libgui_ui_poisoned(u), 0);
    libgui_ui_free(u);}
}

/// A stable id is stable: the same name gives the same number every time,
/// which is what makes it safe to persist and to compare across the boundary.
#[test]
fn ids_from_names_are_stable() {
unsafe {    let a = libgui_id_from_name(c("inspector").as_ptr());
    let b = libgui_id_from_name(c("inspector").as_ptr());
    let d = libgui_id_from_name(c("hierarchy").as_ptr());
    assert_eq!(a, b);
    assert_ne!(a, d);
    assert_eq!(a, libgui::Id::from_name("inspector").0, "the C id disagrees with the Rust one");}
}

// --- custom painting -------------------------------------------------------

static mut PAINTED: u32 = 0;
static mut DROPPED: u32 = 0;

unsafe extern "C" fn paint_cb(p: *mut LibguiPainter, r: LibguiRect, user: *mut c_void) {
    unsafe {
        PAINTED += 1;
        assert_eq!(user as usize, 0xABCD, "the user pointer did not survive the trip");
    }
    libgui_painter_rect(p, r, LibguiColor { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, 2.0);
}

unsafe extern "C" fn drop_cb(_user: *mut c_void) {
    unsafe { DROPPED += 1 };
}

/// **The piece that needed a callback.** A C caller draws its own widget, the
/// painter reaches it, and the data it was given is released afterwards.
#[test]
fn a_c_caller_can_draw_its_own_widget() {
unsafe {    let u = ui();
    let layout = LibguiLayout {
        axis: 0,
        _pad: [0; 3],
        width: LibguiSize { kind: LibguiSizeKind::Fixed, value: 60.0 },
        height: LibguiSize { kind: LibguiSizeKind::Fixed, value: 24.0 },
        pad_left: 0.0,
        pad_right: 0.0,
        pad_top: 0.0,
        pad_bottom: 0.0,
        gap: 0.0,
        align_main: 0,
        align_cross: 0,
        _pad2: [0; 2],
    };
    let id = libgui_id_from_name(c("custom").as_ptr());
    let cb = LibguiPaintFn {
        paint: Some(paint_cb),
        drop_user: Some(drop_cb),
        user: 0xABCD as *mut c_void,
    };
    frame(u, || libgui_add_leaf(u, id, layout, 1, cb));
    frame(u, || libgui_add_leaf(u, id, layout, 1, cb));

    assert!(PAINTED >= 1, "the paint callback never ran");
    assert!(DROPPED >= 1, "the user data was never released");
    libgui_ui_free(u);}
}

/// A callback with no function pointer is a no-op, not a jump through null.
#[test]
fn an_empty_paint_callback_is_harmless() {
unsafe {    let u = ui();
    let layout = LibguiLayout {
        axis: 0,
        _pad: [0; 3],
        width: LibguiSize { kind: LibguiSizeKind::Fixed, value: 10.0 },
        height: LibguiSize { kind: LibguiSizeKind::Fixed, value: 10.0 },
        pad_left: 0.0,
        pad_right: 0.0,
        pad_top: 0.0,
        pad_bottom: 0.0,
        gap: 0.0,
        align_main: 0,
        align_cross: 0,
        _pad2: [0; 2],
    };
    let id = libgui_id_from_name(c("empty").as_ptr());
    let cb = LibguiPaintFn { paint: None, drop_user: None, user: std::ptr::null_mut() };
    frame(u, || libgui_add_leaf(u, id, layout, 0, cb));
    assert_eq!(libgui_ui_poisoned(u), 0);
    libgui_ui_free(u);}
}

/// The disabled scope crosses too, which is what a CAD ribbon needs.
#[test]
fn the_enabled_scope_brackets_over_the_abi() {
unsafe {    let u = ui();
    let label = c("Join");
    frame(u, || {
        let was = libgui_open_enabled(u, 0);
        assert_eq!(libgui_is_enabled(u), 0);
        let r = libgui_button(u, label.as_ptr());
        assert_eq!(r.clicked, 0);
        libgui_close_enabled(u, was);
        assert_eq!(libgui_is_enabled(u), 1);
    });
    libgui_ui_free(u);}
}
