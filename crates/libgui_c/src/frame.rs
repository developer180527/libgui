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
    /// Which of your textures, when `texture_kind` is 1.
    ///
    /// 64 bits, and first, so that it is aligned and so that a native handle
    /// or a pointer survives the trip. It used to be 32, which silently cut
    /// anything wider in half.
    pub texture_index: u64,
    /// 0 = the glyph atlas, 1 = one of your own textures.
    pub texture_kind: u32,
    pub first: u32,
    pub count: u32,
    /// Zero. Named so the struct has no padding a compiler could fill
    /// differently, which is what `libgui_sizeof_batch` is checked against.
    pub _pad: u32,
}

/// One upload's worth of the mesh: the slices to put in a vertex and an index
/// buffer, and which batches to draw from them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibguiMeshChunk {
    /// Into the vertex array, in vertices.
    pub vertex_first: u32,
    pub vertex_count: u32,
    /// Into the index array, in indices. The values there count from
    /// `vertex_first`, so upload the slice and use it as it is.
    pub index_first: u32,
    pub index_count: u32,
    /// Into the batch array. Each batch's `first` counts from `index_first`.
    pub batch_first: u32,
    pub batch_count: u32,
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
    /// Off unless the host asked for it: a renderer that can instance should
    /// keep instancing, and expanding costs about four and a half times the
    /// bytes.
    pub want_mesh: bool,
    /// Reused between frames, so a steady frame expands without allocating.
    pub mesh: libgui::mesh::Mesh,
    /// The mesh's batches, as index ranges rather than instance ranges.
    pub mesh_batches: Vec<LibguiBatch>,
    pub mesh_chunks: Vec<LibguiMeshChunk>,
    /// Per-chunk ceilings, or `u32::MAX` for none.
    pub mesh_limits: (u32, u32),
    /// Off unless the host asked for it: this rasterises the whole frame on
    /// the CPU, which is for checking a renderer, not for shipping.
    pub want_reference: bool,
    /// The reference image, RGBA8, premultiplied, top-left origin.
    pub reference: Vec<u8>,
    pub reference_size: (u32, u32),
    /// Kept between frames: it caches the atlas it has uploaded.
    pub soft: libgui_soft::SoftRenderer,
}

impl Default for FrameData {
    fn default() -> Self {
        Self {
            instances: std::ptr::null(),
            instance_count: 0,
            batches: Vec::new(),
            atlas: std::ptr::null(),
            atlas_size: 0,
            atlas_version: 0,
            globals: LibguiGlobals::default(),
            clear: LibguiColor::default(),
            platform: LibguiPlatformOutput::default(),
            copied: None,
            want_mesh: false,
            mesh: libgui::mesh::Mesh::default(),
            mesh_batches: Vec::new(),
            mesh_chunks: Vec::new(),
            // No ceiling until a host asks for one.
            mesh_limits: (u32::MAX, u32::MAX),
            want_reference: false,
            reference: Vec::new(),
            reference_size: (0, 0),
            soft: libgui_soft::SoftRenderer::new(),
        }
    }
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
        LibguiBatch {
            texture_kind: kind,
            texture_index: index,
            first: b.range.start,
            count: b.range.end - b.range.start,
            _pad: 0,
        }
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

    // The reference image, for a host checking its own renderer against it.
    if into.want_reference {
        let g = out.globals();
        let (w, h) = (
            (g.screen_size[0] * g.scale).round().max(1.0) as u32,
            (g.screen_size[1] * g.scale).round().max(1.0) as u32,
        );
        let target = into.soft.render_to_image(out, w, h);
        into.reference.clear();
        into.reference.extend_from_slice(&target.data);
        into.reference_size = (w, h);
    } else {
        into.reference.clear();
        into.reference_size = (0, 0);
    }

    // The triangle form, for a renderer with no per-instance attributes:
    // bgfx, GLES2, WebGL1, and every RHI that exposes a vertex+index draw and
    // nothing else. Built only when asked for.
    into.mesh_batches.clear();
    into.mesh_chunks.clear();
    if into.want_mesh {
        into.mesh.build_limited(out.draw, into.mesh_limits.0, into.mesh_limits.1);
        into.mesh_chunks.extend(into.mesh.chunks.iter().map(|c| LibguiMeshChunk {
            vertex_first: c.vertices.start,
            vertex_count: c.vertices.end - c.vertices.start,
            index_first: c.indices.start,
            index_count: c.indices.end - c.indices.start,
            batch_first: c.batches.start,
            batch_count: c.batches.end - c.batches.start,
        }));
        into.mesh_batches.extend(into.mesh.batches.iter().map(|b| {
            let (kind, index) = match b.texture {
                libgui::TextureId::Atlas => (0, 0),
                libgui::TextureId::User(i) => (1, i),
            };
            LibguiBatch {
                texture_kind: kind,
                texture_index: index,
                first: b.indices.start,
                count: b.indices.end - b.indices.start,
                _pad: 0,
            }
        }));
    } else {
        into.mesh.vertices.clear();
        into.mesh.indices.clear();
        into.mesh.batches.clear();
        into.mesh.chunks.clear();
    }

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

// ---- the triangle form ---------------------------------------------------

/// Ask for the mesh as well as the instances, from the next `libgui_end_frame`
/// on. Off by default.
///
/// libgui's own output is one instance per primitive, 96 bytes with six vertex
/// attributes, and not every renderer can draw that: bgfx carries at most five
/// vec4s of instance data, GLES2 and WebGL1 have no per-instance attributes at
/// all. With this on, every frame is also expanded into one quad per
/// primitive — four vertices and six indices — which any vertex+index draw can
/// take. It costs about four and a half times the bytes, so a renderer that
/// can instance should keep instancing.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_enable_mesh(ui: *mut LibguiUi, on: u8) {
    if let Some(h) = unsafe { ui.as_mut() } {
        if !h.poisoned {
            h.frame.want_mesh = on != 0;
        }
    }
}

/// The expanded vertices, and how many. Null unless `libgui_enable_mesh` was
/// called before the frame. Valid until the next `libgui_begin_frame`.
///
/// Each is `libgui_vertex_stride` bytes; the fields are in the order
/// `libgui_vertex_attribute` reports.
///
/// # Safety
/// `ui` must be null or a live handle; `out_count` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_mesh_vertices(ui: *mut LibguiUi, out_count: *mut u64) -> *const std::ffi::c_void {
    let (p, n) = crate::handle::with_frame(ui, (std::ptr::null(), 0), |f| {
        (f.mesh.vertices.as_ptr() as *const _, f.mesh.vertices.len() as u64)
    });
    if let Some(slot) = unsafe { out_count.as_mut() } {
        *slot = n;
    }
    p as *const std::ffi::c_void
}

/// The indices, and how many. Always 32-bit; see `libgui_mesh_fits_u16` for
/// when they can be narrowed.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_mesh_indices(ui: *mut LibguiUi, out_count: *mut u64) -> *const u32 {
    let (p, n) = crate::handle::with_frame(ui, (std::ptr::null(), 0), |f| {
        (f.mesh.indices.as_ptr(), f.mesh.indices.len() as u64)
    });
    if let Some(slot) = unsafe { out_count.as_mut() } {
        *slot = n;
    }
    p
}

/// The mesh's batches: the same partition as `libgui_frame_batches`, but
/// `first`/`count` are into the index buffer.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_mesh_batches(ui: *mut LibguiUi, out_count: *mut u64) -> *const LibguiBatch {
    let (p, n) = crate::handle::with_frame(ui, (std::ptr::null(), 0), |f| {
        (f.mesh_batches.as_ptr(), f.mesh_batches.len() as u64)
    });
    if let Some(slot) = unsafe { out_count.as_mut() } {
        *slot = n;
    }
    p
}

/// 1 when every index fits in a `uint16_t`, for a renderer whose index buffers
/// are 16-bit (GLES2, WebGL1, bgfx's default).
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_mesh_fits_u16(ui: *mut LibguiUi) -> u8 {
    crate::handle::with_frame(ui, 0, |f| f.mesh.fits_u16() as u8)
}

/// Bytes per expanded vertex.
#[no_mangle]
pub extern "C" fn libgui_vertex_stride() -> u64 {
    libgui::render_contract::VERTEX_STRIDE as u64
}

/// How many vertex attributes each one has.
#[no_mangle]
pub extern "C" fn libgui_vertex_attribute_count() -> u32 {
    libgui::render_contract::VERTEX_ATTRIBUTES.len() as u32
}

/// What the attribute at `index` *is*: "pos", "local", "uv", "color",
/// "border_color", "clip", "params", "half_size", "seg". Null when out of
/// range.
///
/// Width and offset alone let a host lay out a vertex buffer but not connect
/// it to a shader, which left the mapping hard-coded on the host's side. The
/// name is the shader's varying, so the two can be matched by name instead.
///
/// Owned by the library and valid for the life of the process.
#[no_mangle]
pub extern "C" fn libgui_vertex_attribute_name(index: u32) -> *const std::os::raw::c_char {
    // The names are known at compile time, so each gets its own NUL-terminated
    // literal and nothing is allocated or has to be kept alive by the caller.
    const NAMES: [&str; 9] = [
        "pos\0",
        "local\0",
        "uv\0",
        "color\0",
        "border_color\0",
        "clip\0",
        "params\0",
        "half_size\0",
        "seg\0",
    ];
    match NAMES.get(index as usize) {
        Some(n) => n.as_ptr() as *const std::os::raw::c_char,
        None => std::ptr::null(),
    }
}

/// One vertex attribute: how many floats it is, and its byte offset into the
/// vertex. Returns 0 and writes nothing when `index` is out of range, so a
/// host can describe its vertex layout without hard-coding this one.
///
/// # Safety
/// `out_floats` and `out_offset` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_vertex_attribute(index: u32, out_floats: *mut u32, out_offset: *mut u32) -> u8 {
    let Some(&(_loc, _name, offset, floats)) = libgui::render_contract::VERTEX_ATTRIBUTES.get(index as usize) else {
        return 0;
    };
    if let Some(slot) = unsafe { out_floats.as_mut() } {
        *slot = floats as u32;
    }
    if let Some(slot) = unsafe { out_offset.as_mut() } {
        *slot = offset as u32;
    }
    1
}

// ---- the reference image -------------------------------------------------

/// Render every frame on the CPU as well, so a host can compare its own
/// output against it. Off by default: this rasterises the whole frame in
/// software and is for checking a renderer, not for shipping one.
///
/// See `libgui_conformance_*` for the scene gallery to point it at.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_enable_reference_render(ui: *mut LibguiUi, on: u8) {
    if let Some(h) = unsafe { ui.as_mut() } {
        if !h.poisoned {
            h.frame.want_reference = on != 0;
        }
    }
}

/// The reference image for the last frame: RGBA8, **premultiplied**, top-left
/// origin, tightly packed, `width * height * 4` bytes. Null unless
/// `libgui_enable_reference_render` was on when the frame ended.
///
/// Valid until the next `libgui_begin_frame`.
///
/// A texture of your own draws as nothing here — the CPU renderer has not
/// been given your pixels — so compare scenes that do not use one, or
/// register the same image with both.
///
/// # Safety
/// `ui` must be null or a live handle; the out-parameters null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_reference_pixels(ui: *mut LibguiUi, out_w: *mut u32, out_h: *mut u32) -> *const u8 {
    let (p, w, h) = crate::handle::with_frame(ui, (std::ptr::null(), 0, 0), |f| {
        if f.reference.is_empty() {
            (std::ptr::null(), 0, 0)
        } else {
            (f.reference.as_ptr(), f.reference_size.0, f.reference_size.1)
        }
    });
    if let Some(s) = unsafe { out_w.as_mut() } {
        *s = w;
    }
    if let Some(s) = unsafe { out_h.as_mut() } {
        *s = h;
    }
    p
}

/// Cut each frame's mesh into chunks that fit `max_vertices` and
/// `max_indices`. Zero for either means no limit, which is the default.
///
/// For a renderer streaming into a fixed per-frame buffer. bgfx's transient
/// buffer is 6 MB by default — about thirteen thousand quads — and a frame
/// that goes over it does not run slowly, it loses the draw call. A dense
/// table or a node graph passes that sooner than people expect.
///
/// The limits are rounded down to whole quads (4 vertices, 6 indices), and
/// anything below one quad is treated as one.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_set_mesh_limits(ui: *mut LibguiUi, max_vertices: u32, max_indices: u32) {
    if let Some(h) = unsafe { ui.as_mut() } {
        if !h.poisoned {
            let cap = |v: u32| if v == 0 { u32::MAX } else { v };
            h.frame.mesh_limits = (cap(max_vertices), cap(max_indices));
        }
    }
}

/// The chunks to upload, and how many. One unless a limit forced a split.
///
/// ```c
/// uint64_t n = 0;
/// const LibguiMeshChunk* chunks = libgui_mesh_chunks(ui, &n);
/// for (uint64_t i = 0; i < n; i++) {
///     upload_vertices(vertices + chunks[i].vertex_first, chunks[i].vertex_count);
///     upload_indices(indices + chunks[i].index_first, chunks[i].index_count);
///     for (uint32_t b = 0; b < chunks[i].batch_count; b++) {
///         const LibguiBatch* d = &batches[chunks[i].batch_first + b];
///         draw(d->first, d->count);
///     }
/// }
/// ```
///
/// # Safety
/// `ui` must be null or a live handle; `out_count` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_mesh_chunks(ui: *mut LibguiUi, out_count: *mut u64) -> *const LibguiMeshChunk {
    let (p, n) = crate::handle::with_frame(ui, (std::ptr::null(), 0), |f| {
        (f.mesh_chunks.as_ptr(), f.mesh_chunks.len() as u64)
    });
    if let Some(slot) = unsafe { out_count.as_mut() } {
        *slot = n;
    }
    p
}

