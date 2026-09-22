//! Turning C arguments into Rust ones, without trusting any of them.

use std::os::raw::c_char;

/// A NUL-terminated C string as `&str`, or `None` if it is null or not UTF-8.
///
/// Not a panic and not a silent empty string: a label that arrives as broken
/// bytes is a bug in the caller, and the caller is told through
/// [`crate::libgui_last_error`] while the widget still draws with an empty
/// label rather than taking the frame down.
///
/// # Safety
/// `p` must be null or point to a NUL-terminated string that outlives the call.
pub(crate) unsafe fn str_from<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(p) }.to_str().ok()
}

/// The same, falling back to `""` and recording why.
///
/// # Safety
/// As [`str_from`].
pub(crate) unsafe fn str_or_empty<'a>(p: *const c_char, what: &str) -> &'a str {
    match unsafe { str_from(p) } {
        Some(s) => s,
        None => {
            crate::handle::set_error(&format!("{what}: string is null or not UTF-8"));
            ""
        }
    }
}
