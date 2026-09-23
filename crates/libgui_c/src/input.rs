//! Input in, platform requests out.
//!
//! libgui translates no native events itself — it takes `InputEvent`s and
//! hands back a `PlatformOutput` of requests. Both halves have to cross, or a
//! C host has a UI that draws and does nothing.

use crate::handle::{with_ui, LibguiUi};
use crate::types::LibguiModifiers;
use libgui::{InputEvent, Key, Modifiers, PointerButton, TouchPhase, Vec2, WheelUnit};

/// Pointer buttons, in the order libgui numbers them.
pub const LIBGUI_BUTTON_PRIMARY: u32 = 0;
pub const LIBGUI_BUTTON_SECONDARY: u32 = 1;
pub const LIBGUI_BUTTON_MIDDLE: u32 = 2;
pub const LIBGUI_BUTTON_BACK: u32 = 3;
pub const LIBGUI_BUTTON_FORWARD: u32 = 4;

/// What a wheel delta is measured in. **Do not convert before sending**:
/// libgui applies its own policy per unit, and a host that pre-multiplies
/// lines into pixels loses the distinction a trackpad depends on.
pub const LIBGUI_WHEEL_PIXEL: u32 = 0;
pub const LIBGUI_WHEEL_LINE: u32 = 1;
pub const LIBGUI_WHEEL_PAGE: u32 = 2;

pub const LIBGUI_TOUCH_STARTED: u32 = 0;
pub const LIBGUI_TOUCH_MOVED: u32 = 1;
pub const LIBGUI_TOUCH_ENDED: u32 = 2;
pub const LIBGUI_TOUCH_CANCELLED: u32 = 3;

fn button(b: u32) -> PointerButton {
    match b {
        1 => PointerButton::Secondary,
        2 => PointerButton::Middle,
        3 => PointerButton::Back,
        4 => PointerButton::Forward,
        _ => PointerButton::Primary,
    }
}

/// The pointer moved, in logical pixels relative to this window's content.
///
/// # Safety
/// `ui` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_pointer_moved(ui: *mut LibguiUi, x: f32, y: f32) {
    with_ui(ui, (), |ui| ui.push(InputEvent::PointerMoved { pos: Vec2::new(x, y) }));
}

/// Raw, unaccelerated motion. Only needed while the pointer is locked.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_pointer_delta(ui: *mut LibguiUi, dx: f32, dy: f32) {
    with_ui(ui, (), |ui| ui.push(InputEvent::PointerDelta { delta: Vec2::new(dx, dy) }));
}

/// The pointer left the window.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_pointer_left(ui: *mut LibguiUi) {
    with_ui(ui, (), |ui| ui.push(InputEvent::PointerLeft));
}

/// A pointer button went down or up. See the `LIBGUI_BUTTON_*` constants.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_pointer_button(ui: *mut LibguiUi, btn: u32, pressed: u8) {
    with_ui(ui, (), |ui| {
        ui.push(InputEvent::PointerButton { button: button(btn), pressed: pressed != 0 })
    });
}

/// A wheel or trackpad scroll. `unit` is one of the `LIBGUI_WHEEL_*` constants.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_wheel(ui: *mut LibguiUi, dx: f32, dy: f32, unit: u32) {
    let unit = match unit {
        1 => WheelUnit::Line,
        2 => WheelUnit::Page,
        _ => WheelUnit::Pixel,
    };
    with_ui(ui, (), |ui| ui.push(InputEvent::Wheel { delta: Vec2::new(dx, dy), unit }));
}

/// One finger. `phase` is one of the `LIBGUI_TOUCH_*` constants.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_touch(ui: *mut LibguiUi, id: u64, phase: u32, x: f32, y: f32) {
    let phase = match phase {
        1 => TouchPhase::Move,
        2 => TouchPhase::End,
        3 => TouchPhase::Cancel,
        _ => TouchPhase::Start,
    };
    with_ui(ui, (), |ui| ui.push(InputEvent::Touch { id, phase, pos: Vec2::new(x, y) }));
}

/// A key went down or up. `key` is a `LIBGUI_KEY_*` value from the header.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_key(ui: *mut LibguiUi, key: u32, pressed: u8, repeat: u8) {
    let Some(key) = key_from_code(key) else { return };
    with_ui(ui, (), |ui| {
        ui.push(InputEvent::Key { key, pressed: pressed != 0, repeat: repeat != 0 })
    });
}

/// Modifier state. **Required**: pressing a modifier key does not set it —
/// every windowing library reports modifiers separately, and so does this.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_modifiers(ui: *mut LibguiUi, m: LibguiModifiers) {
    let m = Modifiers { shift: m.shift != 0, ctrl: m.ctrl != 0, alt: m.alt != 0, logo: m.logo != 0 };
    with_ui(ui, (), |ui| ui.push(InputEvent::ModifiersChanged(m)));
}

/// Committed text, already composed by the input method.
///
/// # Safety
/// `text` must be null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_text(ui: *mut LibguiUi, text: *const std::os::raw::c_char) {
    let Some(text) = (unsafe { crate::convert::str_from(text) }) else { return };
    let text = text.to_string();
    with_ui(ui, (), move |ui| ui.push(InputEvent::Text(text)));
}

/// What an input method is composing, and where its caret is (a byte offset).
/// An empty string abandons the composition.
///
/// # Safety
/// `text` must be null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_ime_preedit(
    ui: *mut LibguiUi,
    text: *const std::os::raw::c_char,
    cursor: u64,
) {
    let text = unsafe { crate::convert::str_or_empty(text, "libgui_push_ime_preedit") }.to_string();
    with_ui(ui, (), move |ui| ui.push(InputEvent::ImePreedit { text, cursor: cursor as usize }));
}

/// The clipboard's contents, in answer to `paste_requested`.
///
/// # Safety
/// `text` must be null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_paste(ui: *mut LibguiUi, text: *const std::os::raw::c_char) {
    let Some(text) = (unsafe { crate::convert::str_from(text) }) else { return };
    let text = text.to_string();
    with_ui(ui, (), move |ui| ui.push(InputEvent::Paste(text)));
}

/// The window lost focus.
///
/// # Safety
/// As above.
#[no_mangle]
pub unsafe extern "C" fn libgui_push_focus_lost(ui: *mut LibguiUi) {
    with_ui(ui, (), |ui| ui.push(InputEvent::FocusLost));
}

/// Every key libgui names, numbered for the header. The numbers are this
/// crate's, not libgui's, so reordering `Key` cannot silently change them.
pub(crate) fn key_from_code(code: u32) -> Option<Key> {
    Some(match code {
        1 => Key::ArrowLeft,
        2 => Key::ArrowRight,
        3 => Key::ArrowUp,
        4 => Key::ArrowDown,
        5 => Key::Home,
        6 => Key::End,
        7 => Key::PageUp,
        8 => Key::PageDown,
        9 => Key::Backspace,
        10 => Key::Delete,
        11 => Key::Enter,
        12 => Key::NumpadEnter,
        13 => Key::Tab,
        14 => Key::Escape,
        15 => Key::Space,
        16 => Key::ShiftLeft,
        17 => Key::ShiftRight,
        18 => Key::ControlLeft,
        19 => Key::ControlRight,
        20 => Key::AltLeft,
        21 => Key::AltRight,
        22 => Key::SuperLeft,
        23 => Key::SuperRight,
        // Letters and digits, so a host can bind its own chords.
        c if (100..=125).contains(&c) => LETTERS[(c - 100) as usize],
        c if (130..=139).contains(&c) => DIGITS[(c - 130) as usize],
        c if (140..=151).contains(&c) => FUNCTION[(c - 140) as usize],
        _ => return None,
    })
}

const LETTERS: [Key; 26] = [
    Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L, Key::M, Key::N,
    Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
];
const DIGITS: [Key; 10] = [
    Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9,
];
const FUNCTION: [Key; 12] = [
    Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12,
];
