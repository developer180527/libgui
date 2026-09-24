//! `libgui_units` from C: an expression evaluator with units, for an app that
//! has no grammar of its own to give `libgui_validated_input`.
//!
//! Not libgui. What an expression means is an application's policy, so the
//! evaluator is a companion crate the way key bindings are, and the field
//! libgui owns asks the app rather than deciding. These are here so a C host
//! can use this answer without linking a second library.

use crate::convert::{str_from, str_or_empty};
use crate::handle::set_error;
use crate::text::write_back;
use libgui_units::{Units, Var};
use std::os::raw::c_char;

/// A units table. Opaque: build one with [`libgui_units_length_mm`] or
/// [`libgui_units_new`], and release it with [`libgui_units_free`].
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
