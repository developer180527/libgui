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
use libgui::{TextResponse, Ui};
use std::cell::RefCell;
use std::collections::HashMap;
use std::os::raw::c_char;

thread_local! {
    /// The whole text of a field whose buffer was too small this frame, keyed
    /// by the `Ui`'s address and the field's id.
    ///
    /// Growing the buffer and calling the field again cannot work: a second
    /// call with the same key in one frame is a *different* widget (ids are
    /// disambiguated), one without focus, so it applies nothing and returns
    /// what it was given. And waiting for the next frame cannot work either,
    /// because the next frame is fed from the truncated buffer. The only copy
    /// of the tail is this one.
    ///
    /// Keyed by the `Ui`, not the handle, because inside a dock or table
    /// callback the handle is a temporary one pointing at the same `Ui`.
    /// Thread-local is sound because a `Ui` never leaves its thread.
    static OVERFLOW: RefCell<HashMap<(usize, u64), String>> = RefCell::new(HashMap::new());
}

/// Forget every overflowed field of this `Ui`: at the start of a frame, and
/// when it is freed, so a later `Ui` at the same address inherits nothing.
pub(crate) fn clear_overflow(ui: *const Ui) {
    let key = ui as usize;
    let _ = OVERFLOW.try_with(|m| m.borrow_mut().retain(|k, _| k.0 != key));
}

/// Run a text field over a caller buffer: copy in, edit, copy back, and keep
/// the whole text aside if it did not fit.
unsafe fn field(
    ui: *mut LibguiUi,
    what: &str,
    buf: *mut c_char,
    cap: u64,
    out_len: *mut u64,
    build: impl FnOnce(&mut Ui, &mut String) -> TextResponse,
) -> LibguiTextResponse {
    let mut text = unsafe { str_or_empty(buf as *const c_char, what) }.to_string();
    let mut owner = 0usize;
    let r: LibguiTextResponse = with_ui(ui, LibguiTextResponse::default(), |u| {
        owner = u as *mut Ui as usize;
        build(u, &mut text).into()
    });
    let needed = write_back(&text, buf, cap);
    if let Some(slot) = unsafe { out_len.as_mut() } {
        *slot = needed;
    }
    if needed > cap.saturating_sub(1) && owner != 0 {
        let _ = OVERFLOW.try_with(|m| m.borrow_mut().insert((owner, r.response.id), text));
        if !buf.is_null() {
            set_error(&format!("{what}: buffer too small; fetch the whole text with libgui_text_overflow"));
        }
    }
    r
}

/// The whole text of a field whose buffer was too small, this frame.
///
/// When a text field reports (through `out_len`) more than its buffer held,
/// grow the buffer to that length plus one and call this with the field's
/// `response.id` — **not** the field again, which in the same frame would be a
/// second, unfocused widget. Returns the length, `snprintf`-style; 0 when
/// nothing overflowed.
///
/// ```c
/// uint64_t need = 0;
/// LibguiTextResponse r = libgui_text_input(ui, "path", buf, cap, "", &need);
/// if (need >= cap) {
///     buf = realloc(buf, cap = need + 1);
///     libgui_text_overflow(ui, r.response.id, buf, cap);
/// }
/// ```
///
/// # Safety
/// `ui` null or live; `buf` null or `cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn libgui_text_overflow(ui: *mut LibguiUi, id: u64, buf: *mut c_char, cap: u64) -> u64 {
    let owner = with_ui(ui, 0usize, |u| u as *mut Ui as usize);
    if owner == 0 {
        return 0;
    }
    OVERFLOW
        .try_with(|m| m.borrow().get(&(owner, id)).map(|t| write_back(t, buf, cap)))
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            write_back("", buf, cap);
            0
        })
}

/// Write `s` into `buf`, NUL-terminated, and report what it needed.
///
/// Returns the length the text actually is, not the length written: a caller
/// that gets back more than `cap - 1` knows it was truncated, grows the buffer,
/// and fetches the rest with [`libgui_text_overflow`].
pub(crate) fn write_back(s: &str, buf: *mut c_char, cap: u64) -> u64 {
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
/// the length the text *is* — larger than `cap - 1` means it was truncated:
/// grow the buffer and fetch the whole text with [`libgui_text_overflow`].
/// Calling the field again in the same frame is the tempting fix and the wrong
/// one — see that function.
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
    unsafe { field(ui, "libgui_text_input", buf, cap, out_len, |u, t| u.text_input(key, t, placeholder)) }
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
    unsafe { field(ui, "libgui_text_area", buf, cap, out_len, |u, t| u.text_area(key, t, rows as usize)) }
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

/// Asked, on commit, whether `text` is acceptable. Return 1 to accept. To
/// refuse, return 0 and write the reason into `error` (at most `error_cap`
/// bytes including the NUL; truncation is fine) and, if you know it, the byte
/// offset of the problem into `*error_at`. Leave `*error_at` alone when you do
/// not; it arrives as `UINT64_MAX`.
///
/// `text` is NUL-terminated and also `len` bytes long, and valid only for the
/// call. **Do not call libgui from here** with the same handle: libgui is
/// mid-widget, and such calls are refused.
pub type LibguiValidateFn = Option<
    unsafe extern "C" fn(
        user: *mut std::ffi::c_void,
        text: *const c_char,
        len: u64,
        error: *mut c_char,
        error_cap: u64,
        error_at: *mut u64,
    ) -> u8,
>;

/// How a [`libgui_validated_input`] looks and behaves. Start from
/// [`libgui_validated_options_default`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiValidatedOptions {
    /// Shown while nobody is editing, in place of the source — the `40 mm`
    /// beside a source of `width * 2`. Null shows the source.
    pub display: *const c_char,
    pub placeholder: *const c_char,
    /// Select everything when focus arrives. On by default.
    pub select_on_focus: u8,
    pub _pad: [u8; 7],
    /// Where the field's current refusal is reported, if anywhere. Empty when
    /// there is none.
    pub error: *mut c_char,
    pub error_cap: u64,
}

impl Default for LibguiValidatedOptions {
    fn default() -> Self {
        Self {
            display: std::ptr::null(),
            placeholder: std::ptr::null(),
            select_on_focus: 1,
            _pad: [0; 7],
            error: std::ptr::null_mut(),
            error_cap: 0,
        }
    }
}

/// # Safety
/// `out` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_validated_options_default(out: *mut LibguiValidatedOptions) {
    if let Some(o) = unsafe { out.as_mut() } {
        *o = LibguiValidatedOptions::default();
    }
}

/// Mirrors [`libgui::ValidatedResponse`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiValidatedResponse {
    pub response: crate::types::LibguiResponse,
    /// New text was accepted and written to the buffer this frame.
    pub committed: u8,
    /// The text in the field was edited; the buffer was not.
    pub changed: u8,
    pub cancelled: u8,
    pub focused: u8,
    /// The field holds text the validator refused.
    pub has_error: u8,
    pub _pad: [u8; 3],
    /// Byte offset the validator gave, or `UINT64_MAX` for none.
    pub error_at: u64,
}

impl Default for LibguiValidatedResponse {
    fn default() -> Self {
        Self {
            response: Default::default(),
            committed: 0,
            changed: 0,
            cancelled: 0,
            focused: 0,
            has_error: 0,
            _pad: [0; 3],
            error_at: u64::MAX,
        }
    }
}

/// A text field whose buffer changes only when `validate` accepts the edit.
///
/// `buf` holds the app's **source** text and is written only on an accepted
/// commit — Enter, Tab or a click elsewhere. Escape throws the edit away.
/// Refused text stays in the field with the reason beneath it, and a refused
/// Enter keeps focus with the caret at `error_at`. The grammar, the names and
/// the units are the validator's, which is to say the app's.
///
/// A null `validate` accepts everything, which leaves commit-and-cancel
/// behaviour on its own. `libgui_units_eval` makes a ready-made validator for
/// an app with no grammar of its own.
///
/// `out_len` and a buffer too small for the committed text work exactly as
/// for [`libgui_text_input`]: fetch the rest with [`libgui_text_overflow`].
///
/// # Safety
/// As [`libgui_text_input`]; `opts` null or valid with its pointers as
/// documented; `validate` null or a function honouring
/// [`LibguiValidateFn`]'s contract.
#[no_mangle]
pub unsafe extern "C" fn libgui_validated_input(
    ui: *mut LibguiUi,
    key: *const c_char,
    buf: *mut c_char,
    cap: u64,
    opts: *const LibguiValidatedOptions,
    validate: LibguiValidateFn,
    user: *mut std::ffi::c_void,
    out_len: *mut u64,
) -> LibguiValidatedResponse {
    let what = "libgui_validated_input";
    let key = unsafe { str_or_empty(key, what) };
    let o = unsafe { opts.as_ref() }.copied().unwrap_or_default();
    let display = unsafe { crate::convert::str_from(o.display) };
    let placeholder = unsafe { crate::convert::str_from(o.placeholder) }.unwrap_or("");
    let mut text = unsafe { str_or_empty(buf as *const c_char, what) }.to_string();
    let mut owner = 0usize;
    let r = with_ui(ui, LibguiValidatedResponse::default(), |u| {
        owner = u as *mut Ui as usize;
        let opts = libgui::ValidatedOptions { display, placeholder, select_on_focus: o.select_on_focus != 0 };
        let r = u.validated_input_with(key, &mut text, &opts, |t| {
            let Some(f) = validate else { return Ok(()) };
            let mut cstr = t.as_bytes().to_vec();
            cstr.push(0);
            let mut why = [0 as c_char; 256];
            let mut at = u64::MAX;
            // Everything through this handle is refused while the validator
            // runs: libgui holds `&mut Ui` across the call.
            unsafe { (*ui).validating = true };
            let ok = unsafe { f(user, cstr.as_ptr() as *const c_char, t.len() as u64, why.as_mut_ptr(), why.len() as u64, &mut at) };
            unsafe { (*ui).validating = false };
            if ok != 0 {
                return Ok(());
            }
            *why.last_mut().unwrap() = 0;
            let msg = unsafe { std::ffi::CStr::from_ptr(why.as_ptr()) }.to_string_lossy().into_owned();
            let e = libgui::FieldError::new(if msg.is_empty() { "not accepted".to_string() } else { msg });
            Err(if at == u64::MAX { e } else { e.at(at as usize) })
        });
        write_back(r.error.as_ref().map(|e| e.message.as_str()).unwrap_or(""), o.error, o.error_cap);
        LibguiValidatedResponse {
            response: r.response.into(),
            committed: r.committed as u8,
            changed: r.changed as u8,
            cancelled: r.cancelled as u8,
            focused: r.focused as u8,
            has_error: r.error.is_some() as u8,
            _pad: [0; 3],
            error_at: r.error.as_ref().and_then(|e| e.at).map(|a| a as u64).unwrap_or(u64::MAX),
        }
    });
    let needed = write_back(&text, buf, cap);
    if let Some(slot) = unsafe { out_len.as_mut() } {
        *slot = needed;
    }
    if needed > cap.saturating_sub(1) && owner != 0 {
        let _ = OVERFLOW.try_with(|m| m.borrow_mut().insert((owner, r.response.id), text));
        if !buf.is_null() {
            set_error("libgui_validated_input: buffer too small; fetch the whole text with libgui_text_overflow");
        }
    }
    r
}
