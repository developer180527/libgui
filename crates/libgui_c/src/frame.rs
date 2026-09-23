//! The frame's output: what to draw, and what the host must do about it.
//!
//! Nothing here allocates on the caller's behalf. The pointers returned are
//! into the library's own buffers and are valid **until the next
//! `libgui_begin_frame`** — a host copies them into its vertex buffer during
//! the frame and keeps nothing.

use crate::handle::{with_ui, LibguiUi};
use crate::types::LibguiColor;

/// One batch: a texture and a range of instances to draw with it. Mirrors
/// `libgui::Batch`, whose `Range<u32>` has no C layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibguiBatch {
    /// 0 = the glyph atlas, 1 = one of your own textures.
    pub texture_kind: u32,
    /// Which of your textures, when `texture_kind` is 1.
    pub texture_index: u32,
    pub first: u32,
    pub count: u32,
}

/// The uniform block: target size in logical pixels and the DPI scale.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiGlobals {
    pub screen_width: f32,
    pub screen_height: f32,
    pub scale: f32,
    pub _pad: f32,
}

/// What the host must do after a frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiPlatformOutput {
    /// A `LIBGUI_CURSOR_*` value.
    pub cursor: u32,
    /// Something was copied: read it with `libgui_take_copied_text`.
    pub has_copied_text: u8,
    /// Read the clipboard and send it with `libgui_push_paste`.
    pub paste_requested: u8,
    /// The caret's rect is meaningful: place the IME window or soft keyboard.
    pub has_text_input: u8,
    pub wants_pointer: u8,
    pub wants_keyboard: u8,
    pub pointer_lock: u8,
    /// Negative when nothing is moving: sleep until an event. Zero means draw
    /// again now; a positive value is seconds to wait.
    pub _pad: [u8; 2],
    pub repaint_after: f32,
    pub text_input_x: f32,
    pub text_input_y: f32,
    pub text_input_w: f32,
    pub text_input_h: f32,
}

/// Captured at `libgui_end_frame`, because `FrameOutput` borrows the `Ui` and
/// cannot itself be handed to C.
#[derive(Default)]
pub(crate) struct FrameData {
    pub instances: *const libgui::Instance,
    pub instance_count: u64,
    /// Converted once per frame into a buffer this struct keeps and reuses, so
    /// a steady frame allocates nothing here either.
    pub batches: Vec<LibguiBatch>,
    pub atlas: *const u8,
    pub atlas_size: u32,
    pub atlas_version: u64,
    pub globals: LibguiGlobals,
    pub clear: LibguiColor,
    pub platform: LibguiPlatformOutput,
    pub copied: Option<std::ffi::CString>,
}

pub(crate) fn capture(out: &libgui::FrameOutput, into: &mut FrameData) {
    let g = out.globals();
    into.instances = out.instances().as_ptr();
    into.instance_count = out.instances().len() as u64;
    into.batches.clear();
    into.batches.extend(out.batches().iter().map(|b| {
        let (kind, index) = match b.texture {
            libgui::TextureId::Atlas => (0, 0),
            libgui::TextureId::User(i) => (1, i),
        };
        LibguiBatch { texture_kind: kind, texture_index: index, first: b.range.start, count: b.range.end - b.range.start }
    }));
    let atlas = out.atlas();
    into.atlas = atlas.data.as_ptr();
    into.atlas_size = atlas.size;
    into.atlas_version = atlas.version;
    into.globals = LibguiGlobals {
        screen_width: g.screen_size[0],
        screen_height: g.screen_size[1],
        scale: g.scale,
        _pad: 0.0,
    };
    let c = out.clear_color;
    into.clear = LibguiColor { r: c.r, g: c.g, b: c.b, a: c.a };

    let p = &out.platform;
    into.copied = p.copied_text.as_ref().and_then(|s| std::ffi::CString::new(s.as_str()).ok());
    let r = p.text_input.unwrap_or_default();
    into.platform = LibguiPlatformOutput {
        cursor: p.cursor as u32,
        has_copied_text: into.copied.is_some() as u8,
        paste_requested: p.paste_requested as u8,
        has_text_input: p.text_input.is_some() as u8,
        wants_pointer: p.wants_pointer as u8,
        wants_keyboard: p.wants_keyboard as u8,
        pointer_lock: p.pointer_lock as u8,
        _pad: [0; 2],
        repaint_after: p.repaint_after.unwrap_or(-1.0),
        text_input_x: r.x,
        text_input_y: r.y,
        text_input_w: r.w,
        text_input_h: r.h,
    };
}

/// The instances to draw, and how many. Valid until the next
/// `libgui_begin_frame`; null when no frame has finished.
///
/// # Safety
/// `ui` must be null or a live handle; `out_count` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_frame_instances(ui: *mut LibguiUi, out_count: *mut u64) -> *const std::ffi::c_void {
    let (p, n) = crate::handle::with_frame(ui, (std::ptr::null(), 0), |f| (f.instances as *const _, f.instance_count));
    if let Some(slot) = unsafe { out_count.as_mut() } {
        *slot = n;
    }
    p as *const std::ffi::c_void
}

/// Bytes per instance, so a host can stride its buffer without assuming.
#[no_mangle]
pub extern "C" fn libgui_instance_stride() -> u64 {
    libgui::INSTANCE_STRIDE as u64
}

/// Vertices per instance for the draw call.
#[no_mangle]
pub extern "C" fn libgui_vertices_per_instance() -> u32 {
    libgui::VERTICES_PER_INSTANCE
}

/// The batches, and how many.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_frame_batches(ui: *mut LibguiUi, out_count: *mut u64) -> *const LibguiBatch {
    let (p, n) = crate::handle::with_frame(ui, (std::ptr::null(), 0), |f| (f.batches.as_ptr(), f.batches.len() as u64));
    if let Some(slot) = unsafe { out_count.as_mut() } {
        *slot = n;
    }
    p
}

/// The glyph atlas: a single-channel coverage image, `size` by `size`.
/// Re-upload it when `version` changed.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_frame_atlas(ui: *mut LibguiUi, out_size: *mut u32, out_version: *mut u64) -> *const u8 {
    let (p, s, v) = crate::handle::with_frame(ui, (std::ptr::null(), 0, 0), |f| (f.atlas, f.atlas_size, f.atlas_version));
    if let Some(slot) = unsafe { out_size.as_mut() } {
        *slot = s;
    }
    if let Some(slot) = unsafe { out_version.as_mut() } {
        *slot = v;
    }
    p
}

/// The uniform block.
///
/// # Safety
/// `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_frame_globals(ui: *mut LibguiUi, out: *mut LibguiGlobals) {
    let g = crate::handle::with_frame(ui, LibguiGlobals::default(), |f| f.globals);
    if let Some(slot) = unsafe { out.as_mut() } {
        *slot = g;
    }
}

/// The colour to clear to.
///
/// # Safety
/// `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_frame_clear_color(ui: *mut LibguiUi, out: *mut LibguiColor) {
    let c = crate::handle::with_frame(ui, LibguiColor::default(), |f| f.clear);
    if let Some(slot) = unsafe { out.as_mut() } {
        *slot = c;
    }
}

/// What the host must do: cursor, clipboard, IME, repaint.
///
/// # Safety
/// `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_frame_platform(ui: *mut LibguiUi, out: *mut LibguiPlatformOutput) {
    let p = crate::handle::with_frame(ui, LibguiPlatformOutput::default(), |f| f.platform);
    if let Some(slot) = unsafe { out.as_mut() } {
        *slot = p;
    }
}

/// The text that was copied, as a NUL-terminated string, or null. Owned by the
/// library and valid until the next `libgui_end_frame`.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_frame_copied_text(ui: *mut LibguiUi) -> *const std::os::raw::c_char {
    crate::handle::with_frame(ui, std::ptr::null(), |f| {
        f.copied.as_ref().map_or(std::ptr::null(), |c| c.as_ptr())
    })
}

/// Does anything need drawing, given that `elapsed` seconds have passed since
/// the last frame? False means the host may sleep until the next event.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_needs_frame(ui: *mut LibguiUi, elapsed: f32) -> u8 {
    with_ui(ui, 0, |ui| ui.needs_frame(elapsed) as u8)
}
