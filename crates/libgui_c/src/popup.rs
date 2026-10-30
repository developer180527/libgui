//! Popups, layers and font fallback: the last of what a host needs.

use crate::containers::LibguiFrame;
use crate::handle::{set_error, with_ui, LibguiUi};
use crate::types::LibguiRect;
use libgui::{Frame, Id, Layer, Rect, Theme, Ui};

/// Stacking order for a layer. A modal wants `LIBGUI_LAYER_POPUP` or above, so
/// it sits over the window's own floating panels.
pub const LIBGUI_LAYER_WINDOW: u32 = 0;
pub const LIBGUI_LAYER_POPUP: u32 = 1;
pub const LIBGUI_LAYER_TOOLTIP: u32 = 2;

fn layer_of(z: u32) -> Layer {
    match z {
        1 => Layer::Popup,
        2 => Layer::Tooltip,
        _ => Layer::Window,
    }
}

/// Open a popup anchored to a rect — usually a widget's, so the popup appears
/// beneath it and flips up when there is no room.
///
/// This only *opens* it; build its contents between
/// [`libgui_open_popup_body`] and [`libgui_close_popup_body`] on the frames
/// where that returns 1.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_popup(ui: *mut LibguiUi, id: u64, anchor: LibguiRect) {
    with_ui(ui, (), |u| {
        u.open_popup(Id(id), Rect::new(anchor.x, anchor.y, anchor.w, anchor.h));
    });
}

/// Open `id` as a child of `parent`, keeping `parent` open: a submenu.
///
/// [`libgui_open_popup`] closes whatever was open first, because a second
/// root popup replaces the first. A submenu is the case where it must not,
/// so it has its own call. Nothing happens if `parent` is not open.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_child_popup(
    ui: *mut LibguiUi,
    parent: u64,
    id: u64,
    anchor: LibguiRect,
) {
    with_ui(ui, (), |u| {
        u.open_child_popup(Id(parent), Id(id), Rect::new(anchor.x, anchor.y, anchor.w, anchor.h));
    });
}

/// Begin a popup's contents. Returns 1 when it is open — build the body only
/// then, and call [`libgui_close_popup_body`] after.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_popup_body(ui: *mut LibguiUi, id: u64, min_width: f32) -> u8 {
    with_ui(ui, 0, |u| u.open_popup_body(Id(id), min_width) as u8)
}

/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_popup_body(ui: *mut LibguiUi) {
    with_ui(ui, (), |u| u.close_popup_body());
}

/// Is this popup open?
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_popup_open(ui: *mut LibguiUi, id: u64) -> u8 {
    with_ui(ui, 0, |u| u.popup_open(Id(id)) as u8)
}

/// Is *any* popup or menu open? A host checks this before acting on its own
/// shortcuts, so a chord typed into an open menu does not also fire a command.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_any_popup_open(ui: *mut LibguiUi) -> u8 {
    with_ui(ui, 0, |u| u.any_popup_open() as u8)
}

/// Close one popup.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_popup(ui: *mut LibguiUi, id: u64) {
    with_ui(ui, (), |u| u.close_popup(Id(id)));
}

/// Close every popup — what Escape does, and what a command that opens a
/// dialog should do first.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_popups(ui: *mut LibguiUi) {
    with_ui(ui, (), |u| u.close_popups());
}

/// Open a layer: a container at an explicit rect, above the window's flow
/// content. Close it with [`libgui_close_layer`].
///
/// **This is how you build a modal**, because libgui has none: open a layer
/// covering the window with a translucent fill as the scrim, then a second one
/// at the dialog's rect. What a modal *blocks* is an app's question — whether
/// the menu bar still works, whether Escape cancels — so libgui supplies the
/// stacking and leaves the policy alone.
///
/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_open_layer(
    ui: *mut LibguiUi,
    id: u64,
    z: u32,
    rect: LibguiRect,
    frame: LibguiFrame,
) {
    with_ui(ui, (), |u| {
        u.open_layer(Id(id), layer_of(z), Rect::new(rect.x, rect.y, rect.w, rect.h), Frame::from(frame));
    });
}

/// # Safety
/// `ui` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_close_layer(ui: *mut LibguiUi) {
    with_ui(ui, (), |u| u.close_layer());
}

/// Create a `Ui` with a **fallback chain**: the first font, then the rest for
/// whatever it cannot draw.
///
/// Without one, scripts the first font lacks render as **nothing** — the
/// bundled Inter has no CJK, Arabic, Indic or emoji glyphs, so a name typed in
/// Japanese comes out blank rather than as boxes. Line metrics come from the
/// first face, so adding a CJK fallback does not change the height of a line
/// of Latin.
///
/// `fonts` is an array of `count` pointers, `lens` their lengths. Returns null
/// on failure with the reason in `libgui_last_error`.
///
/// # Safety
/// `fonts` must point to `count` readable buffers of the matching lengths.
#[no_mangle]
pub unsafe extern "C" fn libgui_ui_new_with_fallbacks(
    fonts: *const *const u8,
    lens: *const u64,
    count: u64,
) -> *mut LibguiUi {
    if fonts.is_null() || lens.is_null() || count == 0 {
        set_error("libgui_ui_new_with_fallbacks: no fonts");
        return std::ptr::null_mut();
    }
    let mut owned: Vec<&[u8]> = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let (p, n) = unsafe { (*fonts.add(i), *lens.add(i)) };
        if p.is_null() || n == 0 {
            set_error("libgui_ui_new_with_fallbacks: a font is empty");
            return std::ptr::null_mut();
        }
        owned.push(unsafe { std::slice::from_raw_parts(p, n as usize) });
    }
    match Ui::with_fallbacks(Theme::dark(), &owned) {
        Ok(ui) => crate::handle::into_handle(ui),
        Err(e) => {
            set_error(&format!("libgui_ui_new_with_fallbacks: {e}"));
            std::ptr::null_mut()
        }
    }
}

/// [`libgui_ui_new_with_fallbacks`], then **the fonts installed on the
/// system** for whatever none of yours can draw: a Chinese filename in
/// PingFang or YaHei instead of nothing. Nothing is loaded up front; the
/// first character of a script finds its font (0.1–0.2 s for a CJK one,
/// once). `language` (a BCP-47 tag such as `"ja"` or `"zh-Hant"`, or null)
/// picks which Han glyphs to prefer.
///
/// Null with the reason in `libgui_last_error` on failure, or when this
/// library was built without its `system-fonts` feature.
///
/// # Safety
/// As [`libgui_ui_new_with_fallbacks`]; `language` null or a C string.
#[no_mangle]
pub unsafe extern "C" fn libgui_ui_new_with_system_fonts(
    fonts: *const *const u8,
    lens: *const u64,
    count: u64,
    language: *const std::os::raw::c_char,
) -> *mut LibguiUi {
    let what = "libgui_ui_new_with_system_fonts";
    if fonts.is_null() || lens.is_null() || count == 0 {
        set_error(&format!("{what}: no fonts"));
        return std::ptr::null_mut();
    }
    let mut owned: Vec<&[u8]> = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let (p, n) = unsafe { (*fonts.add(i), *lens.add(i)) };
        if p.is_null() || n == 0 {
            set_error(&format!("{what}: a font is empty"));
            return std::ptr::null_mut();
        }
        owned.push(unsafe { std::slice::from_raw_parts(p, n as usize) });
    }
    let language = unsafe { crate::convert::str_from(language) };
    #[cfg(feature = "system-fonts")]
    {
        let stack = match libgui::FontStack::from_fonts(&owned) {
            Ok(s) => s,
            Err(e) => {
                set_error(&format!("{what}: {e}"));
                return std::ptr::null_mut();
            }
        };
        let mut sys = libgui_sysfonts::SystemFonts::new();
        if let Some(tag) = language {
            sys = sys.with_language(tag);
        }
        crate::handle::into_handle(Ui::with_rasterizer(Theme::dark(), Box::new(stack.with_source(Box::new(sys)))))
    }
    #[cfg(not(feature = "system-fonts"))]
    {
        let _ = (owned, language);
        set_error(&format!("{what}: built without the system-fonts feature"));
        std::ptr::null_mut()
    }
}
