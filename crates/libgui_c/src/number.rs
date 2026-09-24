//! Numbers typed as expressions in units: a CAD dimension box from C.

use crate::convert::{str_from, str_or_empty};
use crate::handle::{set_error, with_ui, LibguiUi};
use crate::text::write_back;
use crate::types::LibguiResponse;
use libgui::{NumberOptions, Units, Var};
use std::os::raw::c_char;

/// A units table. Opaque: build one with [`libgui_units_length_mm`] or
/// [`libgui_units_new`], keep it for as long as the fields that use it, and
/// release it with [`libgui_units_free`].
pub struct LibguiUnits(Units);

fn boxed(u: Units) -> *mut LibguiUnits {
    Box::into_raw(Box::new(LibguiUnits(u)))
}

/// Plain numbers: arithmetic, no units.
#[no_mangle]
pub extern "C" fn libgui_units_none() -> *mut LibguiUnits {
    boxed(Units::none())
}

/// Lengths in millimetres: `mm cm m um µm in " ft '`. Shown in mm.
#[no_mangle]
pub extern "C" fn libgui_units_length_mm() -> *mut LibguiUnits {
    boxed(Units::length_mm())
}

/// Angles in degrees: `deg ° rad`. Shown in degrees.
#[no_mangle]
pub extern "C" fn libgui_units_angle_deg() -> *mut LibguiUnits {
    boxed(Units::angle_deg())
}

/// A table of your own whose base unit is `base`. Null is refused.
///
/// # Safety
/// `base` must be null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn libgui_units_new(base: *const c_char) -> *mut LibguiUnits {
    match unsafe { str_from(base) } {
        Some(b) if !b.is_empty() => boxed(Units::new(b)),
        _ => {
            set_error("libgui_units_new: base must be a non-empty UTF-8 name");
            std::ptr::null_mut()
        }
    }
}

/// Add a unit: one `name` is `factor` base units. A second name with the same
/// factor is an alias. A factor that is not finite and positive is ignored.
///
/// # Safety
/// `units` must be null or from `libgui_units_*`; `name` null or a string.
#[no_mangle]
pub unsafe extern "C" fn libgui_units_add(units: *mut LibguiUnits, name: *const c_char, factor: f64) {
    let (Some(u), Some(n)) = (unsafe { units.as_mut() }, unsafe { str_from(name) }) else { return };
    u.0 = std::mem::take(&mut u.0).with(n, factor);
}

/// Show values in `name`, and read a bare number as `name`.
///
/// # Safety
/// As [`libgui_units_add`].
#[no_mangle]
pub unsafe extern "C" fn libgui_units_set_display(units: *mut LibguiUnits, name: *const c_char) {
    let (Some(u), Some(n)) = (unsafe { units.as_mut() }, unsafe { str_from(name) }) else { return };
    u.0 = std::mem::take(&mut u.0).display(n);
}

/// # Safety
/// `units` must be null or from `libgui_units_*`, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn libgui_units_free(units: *mut LibguiUnits) {
    if !units.is_null() {
        drop(unsafe { Box::from_raw(units) });
    }
}

/// A name an expression may use. `value` is in base units; `dim` is 1 for a
/// quantity of the field's kind (a length) and 0 for a plain number.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiVar {
    pub name: *const c_char,
    pub value: f64,
    pub dim: i32,
    pub _pad: u32,
}

/// Borrow a C var array as Rust vars. Entries whose name is null or not UTF-8
/// are skipped, which makes the expression that uses them fail by name rather
/// than the whole list being refused.
unsafe fn vars_from<'a>(vars: *const LibguiVar, count: u64) -> Vec<Var<'a>> {
    if vars.is_null() || count == 0 {
        return Vec::new();
    }
    let raw = unsafe { std::slice::from_raw_parts(vars, count as usize) };
    raw.iter()
        .filter_map(|v| unsafe { str_from(v.name) }.map(|name| Var { name, value: v.value, dim: v.dim }))
        .collect()
}

/// Write an error's message into a caller buffer, snprintf-style.
fn error_out(message: &str, buf: *mut c_char, cap: u64) {
    write_back(message, buf, cap);
}

/// Evaluate an expression without a field: a command line, a table cell, a
/// script. Returns 1 and writes `*out_value` (in base units) on success;
/// returns 0 and writes the reason into `err`/`err_cap` and its byte offset
/// into `*out_err_at` otherwise. Every output pointer may be null.
///
/// # Safety
/// `units` null or from `libgui_units_*`; `text` null or a string; `vars`
/// null or `var_count` entries; `err` null or `err_cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn libgui_units_eval(
    units: *const LibguiUnits,
    text: *const c_char,
    vars: *const LibguiVar,
    var_count: u64,
    out_value: *mut f64,
    err: *mut c_char,
    err_cap: u64,
    out_err_at: *mut u64,
) -> u8 {
    let Some(u) = (unsafe { units.as_ref() }) else {
        set_error("libgui_units_eval: units is null");
        return 0;
    };
    let text = unsafe { str_or_empty(text, "libgui_units_eval") };
    let vars = unsafe { vars_from(vars, var_count) };
    match u.0.eval(text, &vars) {
        Ok(v) => {
            if let Some(o) = unsafe { out_value.as_mut() } {
                *o = v;
            }
            error_out("", err, err_cap);
            1
        }
        Err(e) => {
            error_out(&e.message, err, err_cap);
            if let Some(o) = unsafe { out_err_at.as_mut() } {
                *o = e.at as u64;
            }
            0
        }
    }
}

/// Format `value` (base units) as a field shows it: `"25.4 mm"`. Returns the
/// length needed, snprintf-style; the text is truncated to fit `cap`.
///
/// # Safety
/// `units` null or from `libgui_units_*`; `buf` null or `cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn libgui_units_format(
    units: *const LibguiUnits,
    value: f64,
    decimals: u32,
    buf: *mut c_char,
    cap: u64,
) -> u64 {
    let Some(u) = (unsafe { units.as_ref() }) else { return 0 };
    write_back(&u.0.format(value, decimals), buf, cap)
}

/// How a [`libgui_number_input`] behaves. Null means the defaults: 3 decimals,
/// no range, no variables. Start from [`libgui_number_options_default`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiNumberOptions {
    pub decimals: u32,
    pub _pad: u32,
    /// Base units. Outside the range is refused with a message, not clamped.
    pub min: f64,
    pub max: f64,
    pub vars: *const LibguiVar,
    pub var_count: u64,
    pub placeholder: *const c_char,
    /// Where the reason goes when the text does not evaluate. May be null.
    pub error: *mut c_char,
    pub error_cap: u64,
}

impl Default for LibguiNumberOptions {
    fn default() -> Self {
        Self {
            decimals: 3,
            _pad: 0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            vars: std::ptr::null(),
            var_count: 0,
            placeholder: std::ptr::null(),
            error: std::ptr::null_mut(),
            error_cap: 0,
        }
    }
}

/// # Safety
/// `out` must be null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_number_options_default(out: *mut LibguiNumberOptions) {
    if let Some(o) = unsafe { out.as_mut() } {
        *o = LibguiNumberOptions::default();
    }
}

/// Mirrors [`libgui::NumberResponse`]. The message is in the caller's buffer
/// named by the options, not here, so nothing is handed back to free.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiNumberResponse {
    pub response: LibguiResponse,
    /// A new value was written this frame: push an undo step, re-solve.
    pub committed: u8,
    /// The text was edited. The value was not: it changes on commit.
    pub changed: u8,
    pub focused: u8,
    /// The field holds text that did not evaluate; the value is untouched.
    pub has_error: u8,
    /// Byte offset of the problem in what was typed.
    pub error_at: u32,
}

/// A number typed as an expression in units: `25.4mm`, `3/8"`, `w/2 + 1cm`.
/// A CAD dimension box.
///
/// `*value` is held in the table's base unit and changes only on commit —
/// Enter, Tab or a click elsewhere. Escape puts back what was there. Text that
/// does not evaluate stays in the field with a red border and the reason
/// beneath it, and `*value` is left alone.
///
/// ```c
/// static LibguiUnits* mm;                 /* libgui_units_length_mm(), once */
/// char why[128];
/// LibguiNumberOptions o;
/// libgui_number_options_default(&o);
/// o.min = 0.0; o.error = why; o.error_cap = sizeof why;
/// if (libgui_number_input(ui, "depth", &depth_mm, mm, &o).committed)
///     push_undo();
/// ```
///
/// # Safety
/// `ui` null or live; `key` a string; `value` null or writable; `units` null or
/// from `libgui_units_*`; `opts` null or valid, with its pointers as documented.
#[no_mangle]
pub unsafe extern "C" fn libgui_number_input(
    ui: *mut LibguiUi,
    key: *const c_char,
    value: *mut f64,
    units: *const LibguiUnits,
    opts: *const LibguiNumberOptions,
) -> LibguiNumberResponse {
    let key = unsafe { str_or_empty(key, "libgui_number_input: key") };
    let Some(value) = (unsafe { value.as_mut() }) else {
        set_error("libgui_number_input: value is null");
        return LibguiNumberResponse::default();
    };
    let none = Units::none();
    let units = unsafe { units.as_ref() }.map(|u| &u.0).unwrap_or(&none);
    let o = unsafe { opts.as_ref() }.copied().unwrap_or_default();
    let vars = unsafe { vars_from(o.vars, o.var_count) };
    let placeholder = unsafe { str_from(o.placeholder) }.unwrap_or("");
    with_ui(ui, LibguiNumberResponse::default(), |u| {
        let opts = NumberOptions { decimals: o.decimals, min: o.min, max: o.max, vars: &vars, placeholder };
        let r = u.number_input_with(key, value, units, &opts);
        match &r.error {
            Some(e) => error_out(&e.message, o.error, o.error_cap),
            None => error_out("", o.error, o.error_cap),
        }
        LibguiNumberResponse {
            response: r.response.into(),
            committed: r.committed as u8,
            changed: r.changed as u8,
            focused: r.focused as u8,
            has_error: r.error.is_some() as u8,
            error_at: r.error.as_ref().map(|e| e.at as u32).unwrap_or(0),
        }
    })
}
