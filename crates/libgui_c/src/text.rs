//! Text fields: the one widget shape the table cannot express.
//!
//! Every other widget takes its state by value or through a small
//! out-parameter. A text field edits a *string*, which grows, and no
//! allocation crosses this boundary — so the caller owns the buffer and says
//! how large it is, `snprintf`-style.
//!
//! The field edits a Rust `String` internally and copies back on the way out.
//! That copy is the price of not allocating across the ABI, and it is bounded
//! by the buffer the caller chose rather than by the document.

use crate::convert::str_or_empty;
use crate::handle::{set_error, with_ui, LibguiUi};
use crate::types::LibguiTextResponse;
use std::os::raw::c_char;

/// Write `s` into `buf`, NUL-terminated, and report what it needed.
///
/// Returns the length the text actually is, not the length written: a caller
/// that gets back more than `cap - 1` knows it was truncated and can grow the
/// buffer and call again next frame, exactly as with `snprintf`.
fn write_back(s: &str, buf: *mut c_char, cap: u64) -> u64 {
    let needed = s.len() as u64;
    if buf.is_null() || cap == 0 {
        return needed;
    }
    let room = (cap - 1) as usize;
    // Truncate on a character boundary: half a UTF-8 sequence is not a string,
    // and the caller will hand this back to us next frame.
    let mut end = room.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr() as *const c_char, buf, end);
        *buf.add(end) = 0;
    }
    needed
}

/// A single-line text field over a buffer the caller owns.
///
/// `buf` holds the text on the way in and is overwritten on the way out;
/// `cap` is its total size including the NUL. `out_len`, if given, receives
/// the length the text *is* — larger than `cap - 1` means it was truncated,
/// so grow the buffer.
///
/// # Safety
/// `buf` must be null or point to `cap` writable bytes holding a
/// NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn libgui_text_input(
    ui: *mut LibguiUi,
    key: *const c_char,
    buf: *mut c_char,
    cap: u64,
    placeholder: *const c_char,
    out_len: *mut u64,
) -> LibguiTextResponse {
    let key = unsafe { str_or_empty(key, "libgui_text_input") };
    let placeholder = unsafe { str_or_empty(placeholder, "libgui_text_input") };
    let mut text = unsafe { str_or_empty(buf as *const c_char, "libgui_text_input") }.to_string();
    let r = with_ui(ui, LibguiTextResponse::default(), |ui| {
        let mut out: LibguiTextResponse = ui.text_input(key, &mut text, placeholder).into();
        // The response mirror carries the field's own `Response` too, which
        // `From<TextResponse>` cannot fill because `TextResponse` does not
        // carry one. Left at its default; use `libgui_interact` on the id if
        // you need hover or focus.
        out.response = Default::default();
        out
    });
    let needed = write_back(&text, buf, cap);
    if let Some(slot) = unsafe { out_len.as_mut() } {
        *slot = needed;
    }
    if needed > cap.saturating_sub(1) && !buf.is_null() {
        set_error("libgui_text_input: buffer too small, text was truncated");
    }
    r
}

/// A multi-line text field, `rows` tall. See [`libgui_text_input`] for how the
/// buffer works.
///
/// # Safety
/// As [`libgui_text_input`].
#[no_mangle]
pub unsafe extern "C" fn libgui_text_area(
    ui: *mut LibguiUi,
    key: *const c_char,
    buf: *mut c_char,
    cap: u64,
    rows: u64,
    out_len: *mut u64,
) -> LibguiTextResponse {
    let key = unsafe { str_or_empty(key, "libgui_text_area") };
    let mut text = unsafe { str_or_empty(buf as *const c_char, "libgui_text_area") }.to_string();
    let r = with_ui(ui, LibguiTextResponse::default(), |ui| {
        let mut out: LibguiTextResponse = ui.text_area(key, &mut text, rows as usize).into();
        out.response = Default::default();
        out
    });
    let needed = write_back(&text, buf, cap);
    if let Some(slot) = unsafe { out_len.as_mut() } {
        *slot = needed;
    }
    if needed > cap.saturating_sub(1) && !buf.is_null() {
        set_error("libgui_text_area: buffer too small, text was truncated");
    }
    r
}

/// A drop-down over a `uint64_t` index the caller owns. `options` is an array
/// of `count` NUL-terminated strings.
///
/// # Safety
/// `options` must point to `count` valid string pointers; `selected` must be
/// null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_combo(
    ui: *mut LibguiUi,
    label: *const c_char,
    selected: *mut u64,
    options: *const *const c_char,
    count: u64,
) -> crate::types::LibguiResponse {
    let label = unsafe { str_or_empty(label, "libgui_combo") };
    let mut owned: Vec<&str> = Vec::with_capacity(count as usize);
    if !options.is_null() {
        for i in 0..count as usize {
            let p = unsafe { *options.add(i) };
            owned.push(unsafe { str_or_empty(p, "libgui_combo option") });
        }
    }
    let mut idx = match unsafe { selected.as_ref() } {
        Some(v) => *v as usize,
        None => 0,
    };
    let r = with_ui(ui, Default::default(), |ui| ui.combo(label, &mut idx, &owned).into());
    if let Some(slot) = unsafe { selected.as_mut() } {
        *slot = idx as u64;
    }
    r
}

/// A one-of-N picker: density, tool modes, view modes.
///
/// # Safety
/// As [`libgui_combo`].
#[no_mangle]
pub unsafe extern "C" fn libgui_segmented(
    ui: *mut LibguiUi,
    key: *const c_char,
    selected: *mut u64,
    options: *const *const c_char,
    count: u64,
) -> crate::types::LibguiResponse {
    let key = unsafe { str_or_empty(key, "libgui_segmented") };
    let mut owned: Vec<&str> = Vec::with_capacity(count as usize);
    if !options.is_null() {
        for i in 0..count as usize {
            let p = unsafe { *options.add(i) };
            owned.push(unsafe { str_or_empty(p, "libgui_segmented option") });
        }
    }
    let mut idx = match unsafe { selected.as_ref() } {
        Some(v) => *v as usize,
        None => 0,
    };
    let r = with_ui(ui, Default::default(), |ui| ui.segmented(key, &mut idx, &owned).into());
    if let Some(slot) = unsafe { selected.as_mut() } {
        *slot = idx as u64;
    }
    r
}
