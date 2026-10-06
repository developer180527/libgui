//! The collection cursor, flattened.
//!
//! `NavResponse` carries what a C caller needs after opening a
//! collection. Returning a struct from the table's shape would need another
//! mirror and another size check, so instead the id comes back and the rest is
//! read through accessors — the same pattern as the frame output.

use crate::handle::{set_error, with_ui, LibguiUi};
use libgui::NavResponse;
use std::cell::Cell;

thread_local! {
    /// The last `open_collection`'s answer. One slot is enough: the accessors
    /// are read straight after the call that fills it, and a collection cannot
    /// be open inside another.
    static LAST: Cell<Option<NavResponse>> = const { Cell::new(None) };
}

pub(crate) fn store(nav: NavResponse) -> u64 {
    LAST.with(|l| l.set(Some(nav)));
    nav.id.0
}

fn get() -> NavResponse {
    LAST.with(|l| l.get()).unwrap_or(NavResponse {
        id: libgui::Id(0),
        cursor: 0,
        moved: false,
        focused: false,
        activated: false,
        expand: false,
        collapse: false,
        extend: false,
    })
}

/// Which item the keyboard cursor is on, after `libgui_open_collection`.
#[no_mangle]
pub extern "C" fn libgui_nav_cursor() -> u64 {
    get().cursor as u64
}

/// The cursor changed this frame. A list that follows its cursor assigns on
/// this rather than every frame, so the pointer can select a different row
/// than the keyboard rests on.
#[no_mangle]
pub extern "C" fn libgui_nav_moved() -> u8 {
    get().moved as u8
}

/// The collection has keyboard focus.
#[no_mangle]
pub extern "C" fn libgui_nav_focused() -> u8 {
    get().focused as u8
}

/// Enter was pressed on the cursor's row: open it, the way a double click
/// would.
#[no_mangle]
pub extern "C" fn libgui_nav_activated() -> u8 {
    get().activated as u8
}

/// A tree's Right. Reported, not acted on — libgui does not know your tree's
/// shape.
#[no_mangle]
pub extern "C" fn libgui_nav_expand() -> u8 {
    get().expand as u8
}

/// A tree's Left.
#[no_mangle]
pub extern "C" fn libgui_nav_collapse() -> u8 {
    get().collapse as u8
}

/// The cursor moved this frame *extending* the selection (Shift with the
/// arrows): grow the range from the anchor — `libgui_select` with kind 2 — rather
/// than replacing the selection.
#[no_mangle]
pub extern "C" fn libgui_nav_extend() -> u8 {
    get().extend as u8
}

/// The label of row `index`, for [`libgui_type_ahead`]: a NUL-terminated
/// UTF-8 string that stays valid until the call returns, or null for a row
/// with none. **Do not call libgui from here.**
pub type LibguiLabelFn = Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, index: u64) -> *const std::os::raw::c_char>;

/// Type-ahead for the collection most recently opened with
/// `libgui_open_collection`: jump its cursor to the row whose label starts
/// with what the user is typing. Call it straight after opening the
/// collection, before building rows; `libgui_nav_cursor` and
/// `libgui_nav_moved` then report the jump.
///
/// Case-insensitive prefix from the current row; the same letter again
/// cycles; a one-second pause starts over; a space that begins a search is
/// the Activate key.
///
/// ```c
/// static const char* part_name(void* parts, uint64_t i) { return ((Part*)parts)[i].name; }
/// libgui_open_collection(ui, "parts", n);
/// libgui_type_ahead(ui, n, part_name, parts);
/// uint64_t cursor = libgui_nav_cursor();
/// ```
///
/// # Safety
/// `ui` null or live; `label` null or honouring [`LibguiLabelFn`].
#[no_mangle]
pub unsafe extern "C" fn libgui_type_ahead(ui: *mut LibguiUi, len: u64, label: LibguiLabelFn, user: *mut std::ffi::c_void) {
    let Some(f) = label else { return };
    let mut nav = get();
    if nav.id.0 == 0 {
        set_error("libgui_type_ahead: no collection has been opened");
        return;
    }
    with_ui(ui, (), |u| {
        // Everything through this handle is refused while labels are asked
        // for: libgui holds `&mut Ui` across the calls.
        unsafe { (*ui).answering = true };
        u.type_ahead(&mut nav, len as usize, |i| {
            let p = unsafe { f(user, i as u64) };
            if p.is_null() {
                std::borrow::Cow::Borrowed("")
            } else {
                unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy()
            }
        });
        unsafe { (*ui).answering = false };
    });
    store(nav);
}
