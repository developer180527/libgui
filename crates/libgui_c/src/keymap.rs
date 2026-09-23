//! Key bindings and the selection convention.
//!
//! libgui binds **no keys**: widgets respond to `UiAction`s and never to keys,
//! because which chord produces one is a platform convention. That is the
//! right split and it means a host that installs nothing has a text field
//! which ignores Backspace. So the defaults have to cross too.

use crate::handle::{set_error, with_ui, LibguiUi};
use crate::types::LibguiModifiers;
use libgui::Modifiers;
use libgui_keymap::{Keymap, Platform};

pub const LIBGUI_PLATFORM_CURRENT: i32 = -1;
pub const LIBGUI_PLATFORM_MAC: i32 = 0;
pub const LIBGUI_PLATFORM_WINDOWS: i32 = 1;
pub const LIBGUI_PLATFORM_LINUX: i32 = 2;

/// `None` for a value that is not a platform.
///
/// Anything unrecognised used to mean "this one", which is the wrong default
/// for the one API whose entire job is that libgui does not guess the
/// platform: a stale constant from an older header, or a field nobody
/// initialised, would silently install Cmd where the caller wanted Ctrl and
/// say nothing. Only `LIBGUI_PLATFORM_CURRENT` asks for the current one.
fn platform(p: i32) -> Option<Platform> {
    match p {
        LIBGUI_PLATFORM_MAC => Some(Platform::Mac),
        LIBGUI_PLATFORM_WINDOWS => Some(Platform::Windows),
        LIBGUI_PLATFORM_LINUX => Some(Platform::Linux),
        LIBGUI_PLATFORM_CURRENT => Some(Platform::current()),
        _ => None,
    }
}

/// Install the default bindings and focus policy for a platform. Pass
/// `LIBGUI_PLATFORM_CURRENT` for the one this binary was built for.
///
/// This also sets the focus policy, which differs: macOS visits text fields
/// only until Full Keyboard Access is on, while Windows and Linux visit every
/// control.
///
/// Returns 0, or 1 if `plat` is not one of the `LIBGUI_PLATFORM_*` values —
/// in which case nothing is installed and `libgui_last_error` says so. Call it
/// once, after `libgui_ui_new`.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_install_keymap(ui: *mut LibguiUi, plat: i32) -> i32 {
    let Some(p) = platform(plat) else {
        crate::handle::set_error("unknown platform: pass a LIBGUI_PLATFORM_* value");
        return 1;
    };
    with_ui(ui, 1, |ui| {
        // `u8` is a stand-in app action: a C host routes its own commands
        // itself, so nothing here needs to know them.
        Keymap::<u8>::new(p).install(ui);
        0
    })
}

/// The bindings for the platform this binary was built for.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_install_default_keymap(ui: *mut LibguiUi) -> i32 {
    unsafe { libgui_install_keymap(ui, LIBGUI_PLATFORM_CURRENT) }
}

/// Which selection gesture a click with these modifiers means: 0 replace,
/// 1 toggle, 2 range.
///
/// Command toggles on macOS and Control everywhere else, and Shift takes a
/// range and wins when both are held. Getting it backwards makes a Mac app
/// feel wrong in a way users notice and cannot name.
#[no_mangle]
pub extern "C" fn libgui_select_kind(plat: i32, m: LibguiModifiers) -> i32 {
    let m = Modifiers { shift: m.shift != 0, ctrl: m.ctrl != 0, alt: m.alt != 0, logo: m.logo != 0 };
    let Some(p) = platform(plat) else {
        crate::handle::set_error("unknown platform: pass a LIBGUI_PLATFORM_* value");
        return -1;
    };
    match libgui_keymap::select_kind(p, &m) {
        libgui::SelectKind::Replace => 0,
        libgui::SelectKind::Toggle => 1,
        libgui::SelectKind::Range => 2,
    }
}

/// Resolve a click on `index` into a change to your selection, keeping the
/// anchor a range-select extends from. Writes the result into `out_*`:
/// `kind` is 0 only, 1 toggle, 2 range; for a range, `lo`..=`hi` inclusive.
///
/// The selection set stays yours — libgui never learns what a row is.
///
/// # Safety
/// `ui` must be null or a live handle; the out-parameters writable or null.
#[no_mangle]
pub unsafe extern "C" fn libgui_select(
    ui: *mut LibguiUi,
    collection: u64,
    index: u64,
    kind: i32,
    out_kind: *mut i32,
    out_lo: *mut u64,
    out_hi: *mut u64,
) {
    let k = match kind {
        1 => libgui::SelectKind::Toggle,
        2 => libgui::SelectKind::Range,
        _ => libgui::SelectKind::Replace,
    };
    let (rk, lo, hi) = with_ui(ui, (0, 0, 0), |ui| match ui.select(libgui::Id(collection), index as usize, k) {
        libgui::Selection::Only(i) => (0, i as u64, i as u64),
        libgui::Selection::Toggle(i) => (1, i as u64, i as u64),
        libgui::Selection::Range(r) => (2, *r.start() as u64, *r.end() as u64),
    });
    if let Some(s) = unsafe { out_kind.as_mut() } {
        *s = rk;
    }
    if let Some(s) = unsafe { out_lo.as_mut() } {
        *s = lo;
    }
    if let Some(s) = unsafe { out_hi.as_mut() } {
        *s = hi;
    }
}

/// Does this chord belong to the app rather than to a focused widget?
///
/// A focused text field takes Cmd+Z for its own undo first and **releases** it
/// when it has nothing left to take back, so the document's undo gets the
/// chord. Build panels first and globals last.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_consume_shortcut(ui: *mut LibguiUi, key: u32, m: LibguiModifiers) -> u8 {
    let Some(key) = crate::input::key_from_code(key) else {
        set_error("libgui_consume_shortcut: unknown key code");
        return 0;
    };
    let mods = Modifiers { shift: m.shift != 0, ctrl: m.ctrl != 0, alt: m.alt != 0, logo: m.logo != 0 };
    with_ui(ui, 0, |ui| ui.consume_shortcut(libgui::Shortcut::new(key, mods)) as u8)
}
