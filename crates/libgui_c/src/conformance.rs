//! The conformance kit: check a renderer against the reference pixels.
//!
//! A library used with unusual renderers — bgfx, a console RHI, an engine's
//! own hardware layer — is ported by hand, and a hand port is *nearly* right.
//! Rounded corners a shade off, a shadow that clips, glyphs half a pixel up at
//! 1.5x DPI: each is invisible next to the reference until someone puts them
//! side by side, and "my port looks slightly off" is not a bug report anyone
//! can act on.
//!
//! So the reference is shipped. `libgui_soft` renders a frame on the CPU by
//! evaluating the same shader per pixel, using only IEEE-exact operations, so
//! it produces the same bytes on every machine. Turn it on, build a frame, and
//! compare:
//!
//! ```c
//! libgui_enable_reference_render(ui, 1);
//! for (uint32_t i = 0; i < libgui_conformance_scene_count(); i++) {
//!     float w, h;
//!     libgui_conformance_scene_size(i, &w, &h);
//!     libgui_begin_frame(ui, w, h, scale, 1.0f);
//!     libgui_conformance_build(ui, i);
//!     libgui_end_frame(ui);
//!
//!     my_renderer_draw(ui);                      /* your backend */
//!     uint32_t rw, rh;
//!     const uint8_t* want = libgui_reference_pixels(ui, &rw, &rh);
//!     compare(my_readback(), want, rw, rh);      /* per scene, pass or fail */
//! }
//! ```
//!
//! The scenes cover every primitive kind: borders, shadows, lines, glyphs,
//! images with rounded corners, clipping, and each at three DPI scales. A
//! failure names the scene, which names the primitive.

use crate::handle::{with_ui, LibguiUi};
use libgui_soft::scenes::{Pointer, SCENES};
use std::cell::RefCell;
use std::ffi::CString;

thread_local! {
    /// The last name handed out, kept alive until the next call. The same
    /// contract as `libgui_last_error`.
    static NAME: RefCell<CString> = RefCell::new(CString::default());
}

/// How many scenes the gallery has.
#[no_mangle]
pub extern "C" fn libgui_conformance_scene_count() -> u32 {
    SCENES.len() as u32
}

/// The scene's name, or null when `index` is out of range. Owned by the
/// library and valid until the next call to this function.
#[no_mangle]
pub extern "C" fn libgui_conformance_scene_name(index: u32) -> *const std::os::raw::c_char {
    let Some(scene) = SCENES.get(index as usize) else { return std::ptr::null() };
    NAME.with(|n| {
        let mut n = n.borrow_mut();
        *n = CString::new(scene.name).unwrap_or_default();
        n.as_ptr()
    })
}

/// The logical size to build the scene at. Its physical size is this times the
/// scale you pass to `libgui_begin_frame`.
///
/// # Safety
/// `out_w` and `out_h` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_conformance_scene_size(index: u32, out_w: *mut f32, out_h: *mut f32) -> u8 {
    let Some(scene) = SCENES.get(index as usize) else { return 0 };
    if let Some(s) = unsafe { out_w.as_mut() } {
        *s = scene.size.0;
    }
    if let Some(s) = unsafe { out_h.as_mut() } {
        *s = scene.size.1;
    }
    1
}

/// 1 when the scene only looks right after input — a hovered button, a drag in
/// flight, a focused field with its caret.
///
/// Those need the pointer or the keyboard driven before the frame that counts,
/// which is the host's to do and differs per toolkit. Skip them for a first
/// port: the rest cover every primitive.
#[no_mangle]
pub extern "C" fn libgui_conformance_scene_needs_input(index: u32) -> u8 {
    SCENES.get(index as usize).is_some_and(|s| s.pointer != Pointer::None) as u8
}

/// Build the scene into the frame in progress. Call it between
/// `libgui_begin_frame` and `libgui_end_frame`, at the size
/// `libgui_conformance_scene_size` reports.
///
/// Build it twice — two frames — before comparing: layout is solved after a
/// frame is built, so anything positioned from last frame's rect is only
/// settled on the second.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_conformance_build(ui: *mut LibguiUi, index: u32) -> u8 {
    let Some(scene) = SCENES.get(index as usize) else { return 0 };
    with_ui(ui, 0, |u| {
        (scene.build)(u);
        1
    })
}
