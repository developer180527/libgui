//! winit adapter for libgui. A template for any other host (SDL, a C++
//! engine, raw HID): translate native events into [`libgui::InputEvent`]s,
//! then apply the [`libgui::PlatformOutput`] after each frame.
//!
//! ```ignore
//! // in window_event:
//! libgui_winit::push_window_event(&mut ui, &event, window.scale_factor());
//! // in device_event (raw, unaccelerated mouse motion for pointer lock):
//! libgui_winit::push_device_event(&mut ui, &event);
//! // each frame:
//! ui.begin_frame(FrameInfo { screen_size, scale, dt });
//! /* build UI */
//! let out = ui.end_frame();
//! platform.apply(&window, &out.platform);
//! ```

use libgui::{Cursor, InputEvent, Key, Modifiers, Payload, PlatformOutput, PointerButton, TouchPhase, Ui, Vec2, WheelUnit};
use std::path::PathBuf;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{DeviceEvent, ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WKey, KeyCode, NamedKey, PhysicalKey};
use winit::window::{CursorGrabMode, CursorIcon, Window};

/// Push the libgui events for one winit window event. Returns true if the
/// event was input (and was pushed).
pub fn push_window_event(ui: &mut Ui, event: &WindowEvent, scale_factor: f64) -> bool {
    let s = scale_factor as f32;
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(position.x as f32 / s, position.y as f32 / s) });
        }
        WindowEvent::CursorLeft { .. } => ui.push(InputEvent::PointerLeft),
        WindowEvent::MouseInput { state, button, .. } => {
            let button = match button {
                MouseButton::Left => PointerButton::Primary,
                MouseButton::Right => PointerButton::Secondary,
                MouseButton::Middle => PointerButton::Middle,
                MouseButton::Back => PointerButton::Back,
                MouseButton::Forward => PointerButton::Forward,
                MouseButton::Other(_) => return false,
            };
            ui.push(InputEvent::PointerButton { button, pressed: *state == ElementState::Pressed });
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let (delta, unit) = match delta {
                MouseScrollDelta::LineDelta(x, y) => (Vec2::new(*x, *y), WheelUnit::Line),
                MouseScrollDelta::PixelDelta(p) => (Vec2::new(p.x as f32 / s, p.y as f32 / s), WheelUnit::Pixel),
            };
            ui.push(InputEvent::Wheel { delta, unit });
        }
        WindowEvent::Touch(t) => {
            let phase = match t.phase {
                winit::event::TouchPhase::Started => TouchPhase::Start,
                winit::event::TouchPhase::Moved => TouchPhase::Move,
                winit::event::TouchPhase::Ended => TouchPhase::End,
                winit::event::TouchPhase::Cancelled => TouchPhase::Cancel,
            };
            let pos = Vec2::new(t.location.x as f32 / s, t.location.y as f32 / s);
            ui.push(InputEvent::Touch { id: t.id, phase, pos });
        }
        WindowEvent::ModifiersChanged(m) => {
            let st = m.state();
            ui.push(InputEvent::ModifiersChanged(Modifiers {
                shift: st.shift_key(),
                ctrl: st.control_key(),
                alt: st.alt_key(),
                logo: st.super_key(),
            }));
        }
        WindowEvent::KeyboardInput { event, .. } => {
            let pressed = event.state == ElementState::Pressed;
            let key = key_from_winit(event.physical_key, &event.logical_key);
            if let Some(key) = key {
                ui.push(InputEvent::Key { key, pressed, repeat: event.repeat });
            }
            if pressed {
                if let Some(text) = &event.text {
                    if key.is_none() && (text == "\n" || text == "\r") {
                        // The iOS keyboard's Return arrives as an unidentified key inserting "\n".
                        ui.push(InputEvent::Key { key: Key::Enter, pressed: true, repeat: false });
                        ui.push(InputEvent::Key { key: Key::Enter, pressed: false, repeat: false });
                    } else {
                        ui.push(InputEvent::Text(text.to_string()));
                    }
                }
            }
        }
        WindowEvent::Ime(Ime::Commit(text)) => ui.push(InputEvent::Text(text.clone())),
        WindowEvent::Focused(false) => ui.push(InputEvent::FocusLost),
        _ => return false,
    }
    true
}

/// Raw mouse motion (unaccelerated on most platforms): drives drags while the
/// pointer is locked.
pub fn push_device_event(ui: &mut Ui, event: &DeviceEvent) {
    if let DeviceEvent::MouseMotion { delta } = event {
        ui.push(InputEvent::PointerDelta { delta: Vec2::new(delta.0 as f32, delta.1 as f32) });
    }
}

/// libgui key for a winit key event. Letters and digits follow the *logical*
/// key, so Cmd/Ctrl+Z stays on the Z key on AZERTY and Dvorak; everything else
/// is the physical key. Falls back to the logical key when the physical one is
/// unknown (e.g. the iOS software keyboard).
pub fn key_from_winit(physical: PhysicalKey, logical: &WKey) -> Option<Key> {
    if let WKey::Character(c) = logical {
        let mut chars = c.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            if let Some(k) = key_from_char(ch) {
                return Some(k);
            }
        }
    }
    if let PhysicalKey::Code(code) = physical {
        if let Some(k) = key_from_code(code) {
            return Some(k);
        }
    }
    match logical {
        WKey::Named(n) => key_from_named(*n),
        _ => None,
    }
}

fn key_from_char(ch: char) -> Option<Key> {
    use Key::*;
    const LETTERS: [Key; 26] = [A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z];
    const DIGITS: [Key; 10] = [Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9];
    match ch.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(LETTERS[(c as u8 - b'a') as usize]),
        c @ '0'..='9' => Some(DIGITS[(c as u8 - b'0') as usize]),
        _ => None,
    }
}

fn key_from_named(n: NamedKey) -> Option<Key> {
    use Key::*;
    Some(match n {
        NamedKey::Enter => Enter,
        NamedKey::Escape => Escape,
        NamedKey::Backspace => Backspace,
        NamedKey::Tab => Tab,
        NamedKey::Space => Space,
        NamedKey::Delete => Delete,
        NamedKey::Insert => Insert,
        NamedKey::Home => Home,
        NamedKey::End => End,
        NamedKey::PageUp => PageUp,
        NamedKey::PageDown => PageDown,
        NamedKey::ArrowLeft => ArrowLeft,
        NamedKey::ArrowRight => ArrowRight,
        NamedKey::ArrowUp => ArrowUp,
        NamedKey::ArrowDown => ArrowDown,
        _ => return None,
    })
}

/// Physical winit key code to libgui key (same US-layout naming as HID).
pub fn key_from_code(code: KeyCode) -> Option<Key> {
    use Key::*;
    Some(match code {
        KeyCode::KeyA => A, KeyCode::KeyB => B, KeyCode::KeyC => C, KeyCode::KeyD => D, KeyCode::KeyE => E,
        KeyCode::KeyF => F, KeyCode::KeyG => G, KeyCode::KeyH => H, KeyCode::KeyI => I, KeyCode::KeyJ => J,
        KeyCode::KeyK => K, KeyCode::KeyL => L, KeyCode::KeyM => M, KeyCode::KeyN => N, KeyCode::KeyO => O,
        KeyCode::KeyP => P, KeyCode::KeyQ => Q, KeyCode::KeyR => R, KeyCode::KeyS => S, KeyCode::KeyT => T,
        KeyCode::KeyU => U, KeyCode::KeyV => V, KeyCode::KeyW => W, KeyCode::KeyX => X, KeyCode::KeyY => Y,
        KeyCode::KeyZ => Z,
        KeyCode::Digit0 => Num0, KeyCode::Digit1 => Num1, KeyCode::Digit2 => Num2, KeyCode::Digit3 => Num3,
        KeyCode::Digit4 => Num4, KeyCode::Digit5 => Num5, KeyCode::Digit6 => Num6, KeyCode::Digit7 => Num7,
        KeyCode::Digit8 => Num8, KeyCode::Digit9 => Num9,
        KeyCode::F1 => F1, KeyCode::F2 => F2, KeyCode::F3 => F3, KeyCode::F4 => F4, KeyCode::F5 => F5,
        KeyCode::F6 => F6, KeyCode::F7 => F7, KeyCode::F8 => F8, KeyCode::F9 => F9, KeyCode::F10 => F10,
        KeyCode::F11 => F11, KeyCode::F12 => F12, KeyCode::F13 => F13, KeyCode::F14 => F14, KeyCode::F15 => F15,
        KeyCode::F16 => F16, KeyCode::F17 => F17, KeyCode::F18 => F18, KeyCode::F19 => F19, KeyCode::F20 => F20,
        KeyCode::F21 => F21, KeyCode::F22 => F22, KeyCode::F23 => F23, KeyCode::F24 => F24,
        KeyCode::Enter => Enter, KeyCode::Escape => Escape, KeyCode::Backspace => Backspace, KeyCode::Tab => Tab,
        KeyCode::Space => Space, KeyCode::Minus => Minus, KeyCode::Equal => Equal,
        KeyCode::BracketLeft => BracketLeft, KeyCode::BracketRight => BracketRight, KeyCode::Backslash => Backslash,
        KeyCode::Semicolon => Semicolon, KeyCode::Quote => Quote, KeyCode::Backquote => Backquote,
        KeyCode::Comma => Comma, KeyCode::Period => Period, KeyCode::Slash => Slash,
        KeyCode::CapsLock => CapsLock, KeyCode::PrintScreen => PrintScreen, KeyCode::ScrollLock => ScrollLock,
        KeyCode::Pause => Pause, KeyCode::Insert => Insert, KeyCode::Delete => Delete, KeyCode::Home => Home,
        KeyCode::End => End, KeyCode::PageUp => PageUp, KeyCode::PageDown => PageDown,
        KeyCode::ArrowRight => ArrowRight, KeyCode::ArrowLeft => ArrowLeft, KeyCode::ArrowDown => ArrowDown,
        KeyCode::ArrowUp => ArrowUp,
        KeyCode::NumLock => NumLock, KeyCode::NumpadDivide => NumpadDivide, KeyCode::NumpadMultiply => NumpadMultiply,
        KeyCode::NumpadSubtract => NumpadSubtract, KeyCode::NumpadAdd => NumpadAdd, KeyCode::NumpadEnter => NumpadEnter,
        KeyCode::NumpadDecimal => NumpadDecimal,
        KeyCode::Numpad0 => Numpad0, KeyCode::Numpad1 => Numpad1, KeyCode::Numpad2 => Numpad2,
        KeyCode::Numpad3 => Numpad3, KeyCode::Numpad4 => Numpad4, KeyCode::Numpad5 => Numpad5,
        KeyCode::Numpad6 => Numpad6, KeyCode::Numpad7 => Numpad7, KeyCode::Numpad8 => Numpad8,
        KeyCode::Numpad9 => Numpad9,
        KeyCode::ContextMenu => ContextMenu,
        KeyCode::ControlLeft => ControlLeft, KeyCode::ShiftLeft => ShiftLeft, KeyCode::AltLeft => AltLeft,
        KeyCode::SuperLeft => SuperLeft, KeyCode::ControlRight => ControlRight, KeyCode::ShiftRight => ShiftRight,
        KeyCode::AltRight => AltRight, KeyCode::SuperRight => SuperRight,
        _ => return None,
    })
}

pub fn cursor_icon(c: Cursor) -> CursorIcon {
    match c {
        Cursor::Default => CursorIcon::Default,
        Cursor::Pointer => CursorIcon::Pointer,
        Cursor::ResizeHorizontal => CursorIcon::EwResize,
        Cursor::ResizeVertical => CursorIcon::NsResize,
        Cursor::ResizeDiagonal => CursorIcon::NwseResize,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::Text => CursorIcon::Text,
    }
}

/// Applies `PlatformOutput` to a window, only touching the OS when a request
/// changes. Clipboard requests are left to the host (it owns the clipboard).
#[derive(Default)]
pub struct PlatformState {
    cursor: Option<Cursor>,
    locked: bool,
    text_input: bool,
    /// Also enable the OS IME on desktop while a text field has focus. Off by
    /// default: with it on, some platforms deliver typing only as IME commits.
    pub desktop_ime: bool,
}

impl PlatformState {
    pub fn apply(&mut self, window: &Window, out: &PlatformOutput) {
        if out.pointer_lock != self.locked {
            self.locked = out.pointer_lock;
            if self.locked {
                // Locked where supported (macOS, Wayland), confined elsewhere (Windows, X11).
                let _ = window.set_cursor_grab(CursorGrabMode::Locked).or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            } else {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
            }
            window.set_cursor_visible(!self.locked);
        }
        if self.cursor != Some(out.cursor) {
            self.cursor = Some(out.cursor);
            window.set_cursor(cursor_icon(out.cursor));
        }
        let mobile = cfg!(any(target_os = "ios", target_os = "android"));
        let want = out.text_input.is_some();
        if want != self.text_input {
            self.text_input = want;
            if mobile || self.desktop_ime {
                window.set_ime_allowed(want);
            }
        }
        if let Some(r) = out.text_input {
            window.set_ime_cursor_area(LogicalPosition::new(r.x, r.y), LogicalSize::new(r.w.max(1.0), r.h));
        }
    }
}

/// Payload kind for a file drag from the OS, carrying `Vec<PathBuf>`.
pub const FILES: &str = "files";

/// Turns winit's per-file drag events into one libgui drag.
///
/// winit reports a file drag one path at a time — `HoveredFile` for each file
/// as it comes over the window, then `DroppedFile` for each when it lands — and
/// none of those events carry a position. That is exactly the kind of
/// platform-shaped detail libgui keeps out of its core: this accumulates the
/// paths and hands the result over with
/// [`Ui::begin_external_drag`](libgui::Ui::begin_external_drag), and the
/// pointer position it routes against is the one the ordinary `CursorMoved`
/// events already established.
///
/// ```ignore
/// // once, next to the Ui:
/// let mut files = FileDrop::default();
/// // in window_event, before push_window_event:
/// files.push_window_event(&mut ui, &event);
/// // in the UI, wherever files are welcome:
/// if let Some(p) = ui.drop_zone(&[libgui_winit::FILES]).dropped {
///     if let Ok(paths) = p.take::<Vec<std::path::PathBuf>>() { /* open them */ }
/// }
/// ```
#[derive(Default)]
pub struct FileDrop {
    /// Paths seen so far in this gesture, in the order winit reported them.
    paths: Vec<PathBuf>,
    /// The drop has been handed over; the remaining `DroppedFile` events for
    /// the same gesture are the tail of it, not a new drag.
    done: bool,
}

impl FileDrop {
    /// Feed it every window event. Returns true if it consumed one, in which
    /// case [`push_window_event`] has nothing to do with it.
    pub fn push_window_event(&mut self, ui: &mut Ui, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::HoveredFile(path) => {
                if self.done {
                    self.reset();
                }
                self.paths.push(path.clone());
                ui.begin_external_drag(Payload::new(FILES, self.paths.clone()).with_label(self.label()));
            }
            WindowEvent::HoveredFileCancelled => {
                self.reset();
                ui.end_external_drag(false);
            }
            WindowEvent::DroppedFile(path) => {
                if self.done {
                    return true;
                }
                // Some platforms drop without ever hovering; then this event is
                // the whole gesture.
                if self.paths.is_empty() {
                    self.paths.push(path.clone());
                    ui.begin_external_drag(Payload::new(FILES, self.paths.clone()).with_label(self.label()));
                }
                self.done = true;
                ui.end_external_drag(true);
            }
            _ => return false,
        }
        true
    }

    fn reset(&mut self) {
        self.paths.clear();
        self.done = false;
    }

    fn label(&self) -> String {
        match self.paths.as_slice() {
            [one] => one.file_name().unwrap_or(one.as_os_str()).to_string_lossy().into_owned(),
            many => format!("{} files", many.len()),
        }
    }
}
