//! The collection cursor, flattened.
//!
//! `NavResponse` carries six things a C caller needs after opening a
//! collection. Returning a struct from the table's shape would need another
//! mirror and another size check, so instead the id comes back and the rest is
//! read through accessors — the same pattern as the frame output.

use crate::handle::{with_ui, LibguiUi};
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

/// Scroll whatever area contains `id` until it is visible. Focus does this for
/// itself; call it for a cursor libgui does not own — a collection's current
/// row, a search hit, a selection made in code.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_scroll_to_id(ui: *mut LibguiUi, id: u64) {
    with_ui(ui, (), |ui| ui.scroll_to(libgui::Id(id)));
}
