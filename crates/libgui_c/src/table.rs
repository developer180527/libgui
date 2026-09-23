//! The widget table: **one declaration, three outputs**.
//!
//! A C API that hand-writes a wrapper per widget does not survive contact with
//! a growing library. Every new widget would be three edits — the Rust export,
//! the header declaration, the symbol list — and the day someone does two of
//! the three, a C++ caller reads the wrong bytes and nothing says so until it
//! crashes somewhere else.
//!
//! So the surface is declared once, here, and the macro emits:
//!
//! 1. the `extern "C"` function, with its null checks, UTF-8 validation and
//!    panic catching;
//! 2. the C declaration for the header (see `header.rs`);
//! 3. an entry in the symbol manifest the drift test reads.
//!
//! **Adding a widget is one line.** It cannot be added to one output and not
//! the others, because there is only one input.
//!
//! The vocabulary is small on purpose. Across forty-odd widgets libgui uses
//! about a dozen parameter types, so each is given a spelling here and every
//! widget is a combination of them.

use crate::convert::str_or_empty;
use crate::handle::{with_ui, LibguiUi};
use crate::types::*;

/// One parameter kind: how it is spelled in C, in Rust, and how a C value
/// becomes the Rust one.
///
/// `$c` is what the header says; `$rust` is the `extern "C"` signature;
/// `$conv` turns the second into what libgui wants.
macro_rules! param {
    (str, $name:ident, $who:expr) => {
        unsafe { str_or_empty($name, $who) }
    };
    (f32, $name:ident, $who:expr) => {
        $name
    };
    (u64, $name:ident, $who:expr) => {
        $name
    };
    (usize, $name:ident, $who:expr) => {
        $name as usize
    };
    (bool, $name:ident, $who:expr) => {
        $name != 0
    };
    // An out-parameter crosses as the raw pointer; the body reads and writes
    // it through `read_*`/`write_*`, which tolerate null.
    (out_f32, $name:ident, $who:expr) => {
        $name
    };
    (out_bool, $name:ident, $who:expr) => {
        $name
    };
    (out_usize, $name:ident, $who:expr) => {
        $name
    };
}

macro_rules! c_type {
    (str) => {
        "const char*"
    };
    (f32) => {
        "float"
    };
    (u64) => {
        "uint64_t"
    };
    (usize) => {
        "uint64_t"
    };
    (bool) => {
        "uint8_t"
    };
    (out_f32) => {
        "float*"
    };
    (out_bool) => {
        "uint8_t*"
    };
    (out_usize) => {
        "uint64_t*"
    };
}

macro_rules! rust_type {
    (str) => { *const std::os::raw::c_char };
    (f32) => { f32 };
    (u64) => { u64 };
    (usize) => { u64 };
    (bool) => { u8 };
    (out_f32) => { *mut f32 };
    (out_bool) => { *mut u8 };
    (out_usize) => { *mut u64 };
}

macro_rules! c_ret {
    (()) => {
        "void"
    };
    (LibguiResponse) => {
        "LibguiResponse"
    };
    (LibguiTreeResponse) => {
        "LibguiTreeResponse"
    };
    (LibguiTextResponse) => {
        "LibguiTextResponse"
    };
    (bool) => {
        "uint8_t"
    };
    (u64) => {
        "uint64_t"
    };
}

/// Declare the widget surface.
///
/// Each line is: the C symbol, the parameters with their kinds, the return
/// type, and the body that calls libgui. The body sees the converted
/// parameters by name and a `ui: &mut Ui`.
macro_rules! widgets {
    // `$ui` is named by the caller, once, so every body below can see it:
    // a binding the macro introduced itself would be hidden by hygiene.
    ($ui:ident => $(
        $(#[$meta:meta])*
        fn $c_name:ident ( $($p:ident : $kind:ident),* $(,)? ) -> $ret:tt $body:block
    )*) => {
        $(
            $(#[$meta])*
            ///
            /// Generated from the table in `table.rs`; see that module.
            ///
            /// # Safety
            /// `ui` must be null or a handle from `libgui_ui_new` that has not
            /// been freed, and any string must be null or NUL-terminated. Null
            /// is tolerated everywhere; a non-null pointer that is not valid
            /// is the one thing no signature can guard against, which is why
            /// this is `unsafe`.
            #[no_mangle]
            pub unsafe extern "C" fn $c_name(
                ui: *mut LibguiUi,
                $($p: rust_type!($kind)),*
            ) -> ret_type!($ret) {
                let who = stringify!($c_name);
                let _ = who;
                with_ui(ui, Default::default(), move |ui| {
                    $(let $p = param!($kind, $p, who);)*
                    let f = |$ui: &mut libgui::Ui| $body;
                    f(ui).into()
                })
            }
        )*

        /// Every generated symbol, its C return type and its C parameters, in
        /// declaration order. `header.rs` turns this into the header and the
        /// drift test compares it against the committed one.
        pub const TABLE: &[(&str, &str, &[(&str, &str)])] = &[
            $((
                stringify!($c_name),
                c_ret!($ret),
                &[$((stringify!($p), c_type!($kind))),*],
            )),*
        ];
    };
}

macro_rules! ret_type {
    (()) => { () };
    (bool) => { u8 };
    (u64) => { u64 };
    ($t:ty) => { $t };
}

// ---------------------------------------------------------------------------
// The surface. One line per widget.
// ---------------------------------------------------------------------------

widgets! { ui =>
    /// A line of body text.
    fn libgui_label(text: str) -> () { ui.label(text) }

    /// Body text in the muted colour.
    fn libgui_label_muted(text: str) -> () { ui.label_muted(text) }

    /// A heading.
    fn libgui_heading(text: str) -> () { ui.heading(text) }

    /// A section header, for grouping a panel's contents.
    fn libgui_section(text: str) -> () { ui.section(text) }

    /// Read-only text that wraps to the width it is given.
    fn libgui_paragraph(text: str) -> () { ui.paragraph(text) }

    /// Fixed empty space along the container's axis.
    fn libgui_space(px: f32) -> () { ui.space(px) }

    /// Space that takes whatever is left: what pushes the next widget to the
    /// far end of a row.
    fn libgui_flex() -> () { ui.flex() }

    /// A rule across the container.
    fn libgui_separator() -> () { ui.separator() }

    /// A button. `clicked` on the response is the thing to check.
    fn libgui_button(label: str) -> LibguiResponse { ui.button(label) }

    /// A button in the accent colour, for the one action a panel is about.
    fn libgui_button_primary(label: str) -> LibguiResponse { ui.button_primary(label) }

    /// A button whose identity is `key` rather than its label, for rows whose
    /// labels repeat.
    fn libgui_button_keyed(key: u64, label: str) -> LibguiResponse { ui.button_keyed(key, label) }

    /// A checkbox over a `uint8_t` the caller owns.
    fn libgui_checkbox(label: str, value: out_bool) -> LibguiResponse {
        let mut v = read_bool(value);
        let r = ui.checkbox(label, &mut v);
        write_bool(value, v);
        r
    }

    /// A switch over a `uint8_t` the caller owns.
    fn libgui_toggle(label: str, value: out_bool) -> LibguiResponse {
        let mut v = read_bool(value);
        let r = ui.toggle(label, &mut v);
        write_bool(value, v);
        r
    }

    /// A slider between `min` and `max` over a `float` the caller owns.
    fn libgui_slider(label: str, value: out_f32, min: f32, max: f32) -> LibguiResponse {
        let mut v = read_f32(value);
        let r = ui.slider(label, &mut v, min, max);
        write_f32(value, v);
        r
    }

    /// A vertical slider of `height` logical pixels.
    fn libgui_slider_vertical(label: str, value: out_f32, min: f32, max: f32, height: f32) -> LibguiResponse {
        let mut v = read_f32(value);
        let r = ui.slider_vertical(label, &mut v, min, max, height);
        write_f32(value, v);
        r
    }

    /// A number you scrub by dragging, at `speed` units per pixel.
    fn libgui_drag_value(label: str, value: out_f32, speed: f32) -> LibguiResponse {
        let mut v = read_f32(value);
        let r = ui.drag_value(label, &mut v, speed);
        write_f32(value, v);
        r
    }

    /// A progress bar. Pass a negative `value` for the indeterminate one.
    fn libgui_progress(label: str, value: f32) -> () {
        ui.progress(label, (value >= 0.0).then_some(value))
    }

    /// A selectable row, for lists and browsers.
    fn libgui_selectable(label: str, selected: bool) -> LibguiResponse {
        ui.selectable(label, selected)
    }

    /// A selectable row identified by `key`, for rows whose labels repeat.
    fn libgui_selectable_keyed(key: u64, label: str, selected: bool) -> LibguiResponse {
        ui.selectable_keyed(key, label, selected)
    }

    /// One row of a menu.
    fn libgui_menu_item(label: str) -> LibguiResponse { ui.menu_item(label) }

    /// A menu row with the chord that performs it shown on the right.
    fn libgui_menu_item_shortcut(label: str, hint: str) -> LibguiResponse {
        ui.menu_item_shortcut(label, hint)
    }

    /// A rule between groups of menu rows.
    fn libgui_menu_separator() -> () { ui.menu_separator() }

    /// Open a menu. Build its items only if this returns 1, and then call
    /// `libgui_close_menu`.
    fn libgui_open_menu(label: str) -> bool { ui.open_menu(label) }

    /// Close the menu opened by `libgui_open_menu`.
    fn libgui_close_menu() -> () { ui.close_menu() }

    /// Scroll whatever area contains this widget until it is visible.
    fn libgui_scroll_to(id: u64) -> () { ui.scroll_to(libgui::Id(id)) }

    /// Whether widgets built now can be used. See `libgui_open_enabled`.
    fn libgui_is_enabled() -> bool { ui.is_enabled() }

    /// One row of a tree. `depth` is the indentation level; `branch` is 0 for a
    /// leaf, 1 for a collapsed branch, 2 for an expanded one.
    ///
    /// `toggled` on the response means the disclosure arrow was hit rather than
    /// the row, and the two are mutually exclusive: a toggle never also selects.
    fn libgui_tree_row(key: u64, depth: usize, branch: usize, label: str, selected: bool) -> LibguiTreeResponse {
        let branch = match branch {
            1 => libgui::Branch::Collapsed,
            2 => libgui::Branch::Expanded,
            _ => libgui::Branch::Leaf,
        };
        ui.tree_row(key, depth, branch, label, selected)
    }

    /// A tooltip on the widget `id`, shown after a hover settles.
    fn libgui_tooltip(id: u64, text: str) -> () {
        let resp = ui.interact(libgui::Id(id));
        ui.tooltip(&resp, text);
    }

    /// Open a context menu for the widget `id`, if it was right-clicked.
    /// Build items only when this returns 1, then call `libgui_close_menu`.
    fn libgui_open_context_menu(id: u64) -> bool {
        let resp = ui.interact(libgui::Id(id));
        ui.open_context_menu(&resp)
    }

    /// A menu row that can be greyed out, with the chord that performs it.
    /// Pass an empty `hint` for none.
    fn libgui_menu_item_ex(label: str, hint: str, enabled: bool) -> LibguiResponse {
        let hint = (!hint.is_empty()).then_some(hint);
        ui.menu_item_ex(label, hint, enabled)
    }

    /// Give a list or tree a keyboard cursor and make it one focus stop
    /// instead of one per row. Close it with `libgui_close_collection`.
    ///
    /// Returns the collection's id; read the cursor with `libgui_nav_*`.
    fn libgui_open_collection(key: str, len: usize) -> u64 {
        let nav = ui.open_collection(key, len);
        crate::nav::store(nav)
    }

    /// Close the collection opened by `libgui_open_collection`.
    fn libgui_close_collection() -> () { ui.close_collection() }

    /// Your own texture, filling the space left in the container: the 3D
    /// view, a render target, a video frame.
    ///
    /// `texture` is the index you register with your renderer; it comes back
    /// in `LibguiBatch::texture_index` with `texture_kind` 1, and drawing it
    /// is the host's job. The response is the one to drive a camera from:
    /// `dragging` with `drag_dx`/`drag_dy` for a tumble, `scroll_y` for dolly.
    ///
    /// For an overlay — a gizmo, a HUD, a selection rectangle — build a
    /// container over it, or use `libgui_add_leaf` and paint into it.
    ///
    /// It *grows* to fill what it is given, so its container must have a size
    /// to give: inside one whose height is `Fit`, a viewport is zero pixels
    /// tall and draws nothing at all.
    fn libgui_viewport(key: str, texture: u64) -> LibguiResponse {
        ui.viewport(key, libgui::TextureId::User(texture as u32), |_, _| {})
    }

    /// Put the keyboard cursor on `index`, so clicking a row leaves it where
    /// the pointer left off.
    fn libgui_set_cursor(collection: u64, index: usize) -> () {
        ui.set_cursor(libgui::Id(collection), index)
    }
}

fn read_bool(p: *mut u8) -> bool {
    match unsafe { p.as_ref() } {
        Some(v) => *v != 0,
        None => false,
    }
}

fn write_bool(p: *mut u8, v: bool) {
    if let Some(slot) = unsafe { p.as_mut() } {
        *slot = v as u8;
    }
}

fn read_f32(p: *mut f32) -> f32 {
    match unsafe { p.as_ref() } {
        Some(v) => *v,
        None => 0.0,
    }
}

fn write_f32(p: *mut f32, v: f32) {
    if let Some(slot) = unsafe { p.as_mut() } {
        *slot = v;
    }
}
