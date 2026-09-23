//! Loading a theme from TOML, and drag and drop.

use crate::convert::{str_from, str_or_empty};
use crate::handle::{set_error, with_ui, LibguiUi};
use crate::types::LibguiRect;
use libgui::{Payload, Theme};
use std::collections::HashSet;
use std::os::raw::c_char;
use std::sync::Mutex;

/// Load a theme from a TOML string. Returns 0 on success, 1 on failure with
/// the reason in `libgui_last_error`.
///
/// This is how an existing palette ports without retyping a colour: export it
/// once from whatever the app already has, and hand the text over.
///
/// # Safety
/// `toml` must be null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_set_theme_toml(ui: *mut LibguiUi, toml: *const c_char) -> i32 {
    let Some(text) = (unsafe { str_from(toml) }) else {
        set_error("libgui_set_theme_toml: null or not UTF-8");
        return 1;
    };
    match Theme::from_toml(text) {
        Ok(t) => with_ui(ui, 1, |ui| {
            ui.theme = t;
            0
        }),
        Err(e) => {
            // The error names the section and the field, which is most of the
            // value of having a theme file at all.
            set_error(&format!("libgui_set_theme_toml: {e}"));
            1
        }
    }
}

/// Write the current theme as TOML into a buffer the caller owns.
///
/// `snprintf`'s contract: returns the length needed, so call with `cap` 0 to
/// size the buffer. Every value resolved, which makes it a reference to copy
/// from rather than a diff against a preset.
///
/// # Safety
/// `buf` must be null or point to `cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn libgui_theme_to_toml(ui: *mut LibguiUi, buf: *mut c_char, cap: u64) -> i64 {
    with_ui(ui, -1, |ui| {
        let text = ui.theme.to_toml();
        let needed = text.len() as i64;
        if !buf.is_null() && cap > 0 {
            let room = (cap - 1) as usize;
            let mut end = room.min(text.len());
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(text.as_ptr() as *const c_char, buf, end);
                *buf.add(end) = 0;
            }
        }
        needed
    })
}

// ---------------------------------------------------------------------------
// Drag and drop
// ---------------------------------------------------------------------------

/// `Payload::new` wants a `&'static str` for the kind, and a C string is not
/// static. Kinds are interned here instead: an app has a handful of them, they
/// are leaked once, and the same text always gives the same pointer.
static KINDS: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

fn intern(kind: &str) -> &'static str {
    let mut guard = KINDS.lock().unwrap_or_else(|e| e.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    if let Some(k) = set.get(kind) {
        return k;
    }
    let leaked: &'static str = Box::leak(kind.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// What a drop zone saw this frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiDropZone {
    /// A drag this zone accepts is over it now.
    pub hovered: u8,
    /// Something was dropped this frame: `value` and `kind_id` say what.
    pub dropped: u8,
    pub _pad: [u8; 6],
    /// The payload's value, as passed to `libgui_drag_source`.
    pub value: u64,
    pub rect: LibguiRect,
    /// Where the pointer was, in this zone's coordinates.
    pub pointer_x: f32,
    pub pointer_y: f32,
}

/// Make the widget `id` draggable, carrying `kind` and `value`.
///
/// `value` is yours to interpret — a body id, a row index, a pointer you cast.
/// libgui carries it and hands it back at the drop.
///
/// Returns 1 while this widget is the one being dragged.
///
/// # Safety
/// `kind` and `label` must be null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_drag_source(
    ui: *mut LibguiUi,
    id: u64,
    kind: *const c_char,
    value: u64,
    label: *const c_char,
) -> u8 {
    let kind = intern(unsafe { str_or_empty(kind, "libgui_drag_source") });
    let label = unsafe { str_or_empty(label, "libgui_drag_source") }.to_string();
    with_ui(ui, 0, move |ui| {
        let d = ui.drag_source(libgui::Id(id), || Payload::new(kind, value).with_label(label));
        d.dragging as u8
    })
}

/// Accept drags of the given kinds over the container being built.
///
/// # Safety
/// `kinds` must point to `count` NUL-terminated strings; `out` writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_drop_zone(
    ui: *mut LibguiUi,
    kinds: *const *const c_char,
    count: u64,
    out: *mut LibguiDropZone,
) {
    let mut owned: Vec<&str> = Vec::with_capacity(count as usize);
    if !kinds.is_null() {
        for i in 0..count as usize {
            let p = unsafe { *kinds.add(i) };
            owned.push(unsafe { str_or_empty(p, "libgui_drop_zone kind") });
        }
    }
    let z = with_ui(ui, LibguiDropZone::default(), move |ui| {
        let zone = ui.drop_zone(&owned);
        let dropped = zone.dropped.as_ref();
        LibguiDropZone {
            hovered: zone.hovered as u8,
            dropped: dropped.is_some() as u8,
            _pad: [0; 6],
            value: dropped.and_then(|p| p.get::<u64>().copied()).unwrap_or(0),
            rect: zone.rect.into(),
            pointer_x: zone.pointer.x,
            pointer_y: zone.pointer.y,
        }
    });
    if let Some(slot) = unsafe { out.as_mut() } {
        *slot = z;
    }
}

/// What kind of thing is being dragged, or null. The string is interned and
/// stays valid for the life of the process.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dragging(ui: *mut LibguiUi) -> *const c_char {
    let kind = with_ui(ui, None, |ui| ui.dragging());
    match kind {
        // Interning gives a stable NUL-terminated copy, since libgui's own
        // `&'static str` is not NUL-terminated.
        Some(k) => nul_terminated(k),
        None => std::ptr::null(),
    }
}

/// Abandon the drag in flight — what Escape should do.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_cancel_drag(ui: *mut LibguiUi) {
    with_ui(ui, (), |ui| ui.cancel_drag());
}

/// A NUL-terminated copy of an interned kind, leaked once per distinct kind so
/// the pointer a caller keeps stays valid.
static C_KINDS: Mutex<Option<std::collections::HashMap<&'static str, &'static std::ffi::CStr>>> = Mutex::new(None);

fn nul_terminated(k: &'static str) -> *const c_char {
    let mut guard = C_KINDS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(Default::default);
    if let Some(c) = map.get(k) {
        return c.as_ptr();
    }
    let Ok(owned) = std::ffi::CString::new(k) else {
        return std::ptr::null();
    };
    let leaked: &'static std::ffi::CStr = Box::leak(owned.into_boxed_c_str());
    map.insert(k, leaked);
    leaked.as_ptr()
}
