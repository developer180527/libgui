//! The colour picker and the swatch that opens one.

use crate::convert::str_or_empty;
use crate::handle::{with_ui, LibguiUi};
use crate::types::LibguiColor;
use libgui::{Color, ColorPickerOptions, ColorPickerResponse};
use std::os::raw::c_char;

/// Mirrors [`libgui::ColorPickerOptions`]. Start from
/// [`libgui_color_picker_options_default`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiColorPickerOptions {
    /// Offer an alpha strip.
    pub alpha: u8,
    /// Show the hex field.
    pub hex: u8,
    pub _pad: [u8; 2],
    /// Height of the saturation/value square, logical px.
    pub square_height: f32,
}

impl From<LibguiColorPickerOptions> for ColorPickerOptions {
    fn from(o: LibguiColorPickerOptions) -> Self {
        Self { alpha: o.alpha != 0, hex: o.hex != 0, square_height: o.square_height }
    }
}

/// # Safety
/// `out` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_color_picker_options_default(out: *mut LibguiColorPickerOptions) {
    if let Some(o) = unsafe { out.as_mut() } {
        let d = ColorPickerOptions::default();
        *o = LibguiColorPickerOptions { alpha: d.alpha as u8, hex: d.hex as u8, _pad: [0; 2], square_height: d.square_height };
    }
}

/// Mirrors [`libgui::ColorPickerResponse`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiColorPickerResponse {
    /// The colour changed this frame — every frame of a drag.
    pub changed: u8,
    /// An edit finished: a drag let go, or a hex value committed. Push an undo
    /// step on this one.
    pub finished: u8,
    pub dragging: u8,
    pub _pad: u8,
}

impl From<ColorPickerResponse> for LibguiColorPickerResponse {
    fn from(r: ColorPickerResponse) -> Self {
        Self { changed: r.changed as u8, finished: r.finished as u8, dragging: r.dragging as u8, _pad: 0 }
    }
}

unsafe fn edit(
    ui: *mut LibguiUi,
    key: *const c_char,
    color: *mut LibguiColor,
    opts: *const LibguiColorPickerOptions,
    what: &str,
    run: impl FnOnce(&mut libgui::Ui, &str, &mut Color, ColorPickerOptions) -> ColorPickerResponse,
) -> LibguiColorPickerResponse {
    let key = unsafe { str_or_empty(key, what) };
    let Some(c) = (unsafe { color.as_mut() }) else {
        crate::handle::set_error(&format!("{what}: color is null"));
        return LibguiColorPickerResponse::default();
    };
    let o = unsafe { opts.as_ref() }.map(|o| ColorPickerOptions::from(*o)).unwrap_or_default();
    with_ui(ui, LibguiColorPickerResponse::default(), |u| {
        let mut col = Color::rgba(c.r, c.g, c.b, c.a);
        let r = run(u, key, &mut col, o);
        *c = LibguiColor { r: col.r, g: col.g, b: col.b, a: col.a };
        r.into()
    })
}

/// A colour picker editing `*color` in place: a saturation/value square, a
/// hue strip, an optional alpha strip and a hex field. `opts` may be null.
///
/// ```c
/// if (libgui_color_picker(ui, "layer", &layer_color, NULL).finished)
///     push_undo();
/// ```
///
/// # Safety
/// `ui` null or live; `key` a string; `color` null or writable; `opts` null or
/// valid.
#[no_mangle]
pub unsafe extern "C" fn libgui_color_picker(
    ui: *mut LibguiUi,
    key: *const c_char,
    color: *mut LibguiColor,
    opts: *const LibguiColorPickerOptions,
) -> LibguiColorPickerResponse {
    unsafe { edit(ui, key, color, opts, "libgui_color_picker", |u, k, c, o| u.color_picker_with(k, c, o)) }
}

/// A swatch of `*color` that opens a picker in a popup when clicked: what an
/// inspector row shows. `opts` configures the picker it opens; may be null.
///
/// # Safety
/// As [`libgui_color_picker`].
#[no_mangle]
pub unsafe extern "C" fn libgui_color_button(
    ui: *mut LibguiUi,
    key: *const c_char,
    color: *mut LibguiColor,
    opts: *const LibguiColorPickerOptions,
) -> LibguiColorPickerResponse {
    unsafe { edit(ui, key, color, opts, "libgui_color_button", |u, k, c, o| u.color_button_with(k, c, o)) }
}
