//! Modal dialogs.

use crate::convert::str_or_empty;
use crate::handle::{with_ui, LibguiUi};
use libgui::{ModalOptions, ModalResponse};
use std::os::raw::c_char;

/// Mirrors [`libgui::ModalOptions`]. Start from [`libgui_modal_options_default`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiModalOptions {
    /// The dialog's width, logical px; its height fits its content.
    pub width: f32,
    /// Dim the window behind (1 by default). Off, it is still blocked.
    pub dim: u8,
    /// Let app shortcuts built outside the dialog fire while it is up (0).
    pub shortcuts_behind: u8,
    pub _pad: [u8; 2],
}

impl From<LibguiModalOptions> for ModalOptions {
    fn from(o: LibguiModalOptions) -> Self {
        Self { width: o.width, dim: o.dim != 0, shortcuts_behind: o.shortcuts_behind != 0 }
    }
}

/// # Safety
/// `out` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_modal_options_default(out: *mut LibguiModalOptions) {
    if let Some(o) = unsafe { out.as_mut() } {
        let d = ModalOptions::default();
        *o = LibguiModalOptions { width: d.width, dim: d.dim as u8, shortcuts_behind: d.shortcuts_behind as u8, _pad: [0; 2] };
    }
}

/// Mirrors [`libgui::ModalResponse`]. Nothing here closes the dialog: stop
/// building it and it is gone.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiModalResponse {
    /// Escape, while this dialog is on top.
    pub cancelled: u8,
    /// Enter that no control inside used: the dialog's default action.
    pub submitted: u8,
    /// A click on the dimmed window outside the dialog.
    pub clicked_outside: u8,
    /// The first frame it is shown.
    pub opened: u8,
}

impl From<ModalResponse> for LibguiModalResponse {
    fn from(r: ModalResponse) -> Self {
        Self { cancelled: r.cancelled as u8, submitted: r.submitted as u8, clicked_outside: r.clicked_outside as u8, opened: r.opened as u8 }
    }
}

/// Open a modal dialog titled `title` (NULL or "" for none). Build its body,
/// then call [`libgui_close_modal`]. `opts` may be null.
///
/// # Safety
/// `ui` null or live; `key` and `title` null or strings; `opts` null or valid.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_modal(ui: *mut LibguiUi, key: *const c_char, title: *const c_char, opts: *const LibguiModalOptions) {
    let key = unsafe { str_or_empty(key, "libgui_open_modal") };
    let title = if title.is_null() { "" } else { unsafe { str_or_empty(title, "libgui_open_modal") } };
    let o = unsafe { opts.as_ref() }.map(|o| ModalOptions::from(*o)).unwrap_or_default();
    with_ui(ui, (), |u| u.open_modal(key, title, &o));
}

/// Close the dialog opened by [`libgui_open_modal`], and say what happened
/// to it this frame. Closing with none open, or with a container still open
/// inside it, poisons the handle, as an unbalanced `close_container` does.
///
/// # Safety
/// `ui` null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_modal(ui: *mut LibguiUi) -> LibguiModalResponse {
    with_ui(ui, LibguiModalResponse::default(), |u| u.close_modal().into())
}

/// 1 while a modal dialog is up: what a host checks before acting on input
/// it reads itself.
///
/// # Safety
/// `ui` null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_any_modal_open(ui: *mut LibguiUi) -> u8 {
    with_ui(ui, 0, |u| u.any_modal_open() as u8)
}
