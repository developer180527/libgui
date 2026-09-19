//! Input: hosts push [`InputEvent`]s from any source (winit, SDL, UIKit,
//! Android, raw HID…) with [`crate::Ui::push`]; libgui turns them into a
//! per-frame [`FrameInput`] when the frame begins. Nothing here knows about a
//! platform: a host's only job is translating its native events.

use crate::{Rect, Vec2};

/// Everything the host knows about the frame that is not an event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameInfo {
    /// Drawable size in logical px.
    pub screen_size: Vec2,
    /// Physical px per logical px.
    pub scale: f32,
    /// Seconds since the previous frame.
    pub dt: f32,
}

impl Default for FrameInfo {
    fn default() -> Self {
        Self { screen_size: Vec2::new(1280.0, 800.0), scale: 1.0, dt: 1.0 / 60.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// Left mouse button, pen tip, or a finger (touch uses `Touch` events).
    Primary,
    /// Right button: context menus.
    Secondary,
    /// Middle button / wheel click: CAD-style panning.
    Middle,
    Back,
    Forward,
}

impl PointerButton {
    pub const ALL: [PointerButton; 5] =
        [PointerButton::Primary, PointerButton::Secondary, PointerButton::Middle, PointerButton::Back, PointerButton::Forward];

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelUnit {
    /// Precise deltas (trackpads), logical px.
    Pixel,
    /// Mouse wheel notches.
    Line,
    /// Page up/down steps.
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    Start,
    Move,
    End,
    /// The OS took the touch away (e.g. a system gesture): no tap fires.
    Cancel,
}

/// One input event. Push them in the order they happened; several per frame
/// is normal. Positions are logical px in the window/drawable space.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// Absolute pointer position (OS cursor, pen hover, or a `VirtualCursor`).
    PointerMoved { pos: Vec2 },
    /// Relative motion, unaccelerated if the source is raw (HID, raw input).
    /// Used for drags while the pointer is locked; see `PlatformOutput::pointer_lock`.
    PointerDelta { delta: Vec2 },
    /// The pointer left the window.
    PointerLeft,
    PointerButton { button: PointerButton, pressed: bool },
    /// Positive `y` scrolls content down (wheel away from the user).
    Wheel { delta: Vec2, unit: WheelUnit },
    Touch { id: u64, phase: TouchPhase, pos: Vec2 },
    /// A physical key (layout-independent, like a USB HID usage). Text comes
    /// separately as `Text`, so shortcuts and typing never interfere.
    Key { key: Key, pressed: bool, repeat: bool },
    /// Replace the modifier state (hosts that track modifiers themselves).
    /// Hosts that only send `Key` events get modifiers derived from them.
    ModifiersChanged(Modifiers),
    /// Committed text: typed characters, IME commits, dictation.
    Text(String),
    /// Clipboard contents, typically sent in response to `PlatformOutput::paste_requested`.
    Paste(String),
    /// Explicit copy/cut (e.g. from an OS menu). Cmd/Ctrl+C/X are handled from `Key` events.
    Copy,
    Cut,
    /// The window lost focus: every button and key is released.
    FocusLost,
}

/// Modifier state. `shift`/`ctrl`/`alt`/`logo` are the real keys; `command`
/// and `word` are what they mean on this platform (see [`Modifiers::from_keys`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Cmd on macOS, Windows key elsewhere.
    pub logo: bool,
    /// Shortcut modifier: Cmd on Apple platforms, Ctrl elsewhere.
    pub command: bool,
    /// Word-wise caret movement: Option on Apple platforms, Ctrl elsewhere.
    pub word: bool,
}

impl Modifiers {
    pub fn from_keys(shift: bool, ctrl: bool, alt: bool, logo: bool, mac: bool) -> Self {
        Self {
            shift,
            ctrl,
            alt,
            logo,
            command: if mac { logo } else { ctrl },
            word: if mac { alt } else { ctrl },
        }
    }
}

/// Physical keys, named after the US layout (like USB HID usages and the web's
/// `KeyboardEvent.code`). Convert from raw HID with [`Key::from_hid_usage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    Enter, Escape, Backspace, Tab, Space,
    Minus, Equal, BracketLeft, BracketRight, Backslash, Semicolon, Quote, Backquote, Comma, Period, Slash,
    CapsLock, PrintScreen, ScrollLock, Pause, Insert, Delete, Home, End, PageUp, PageDown,
    ArrowRight, ArrowLeft, ArrowDown, ArrowUp,
    NumLock, NumpadDivide, NumpadMultiply, NumpadSubtract, NumpadAdd, NumpadEnter, NumpadDecimal,
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    ContextMenu,
    ControlLeft, ShiftLeft, AltLeft, SuperLeft, ControlRight, ShiftRight, AltRight, SuperRight,
}

/// A keyboard shortcut, written in terms of what the modifiers *mean* rather
/// than which keys they are: `command` is Cmd on Apple platforms and Ctrl
/// elsewhere, so one declaration is right on both.
///
/// libgui does not define what a shortcut *does* — that is your app's keymap,
/// and your users will want to rebind it. What libgui provides is matching,
/// platform-correct modifiers, routing by focus, and consumption so a
/// shortcut cannot fire twice. See [`crate::Ui::consume_shortcut`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub key: Key,
    /// Cmd on Apple platforms, Ctrl elsewhere.
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    /// A bare key: `F2`, `Delete`.
    pub const fn plain(key: Key) -> Self {
        Self { key, command: false, shift: false, alt: false }
    }

    /// Cmd+key on Apple platforms, Ctrl+key elsewhere.
    pub const fn command(key: Key) -> Self {
        Self { key, command: true, shift: false, alt: false }
    }

    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Exact match: `Cmd+S` must not fire on `Cmd+Shift+S`, and on a Mac
    /// `Ctrl+S` must not fire a `command` shortcut.
    pub fn matches(&self, m: &Modifiers, mac: bool) -> bool {
        let (primary, other) = if mac { (m.logo, m.ctrl) } else { (m.ctrl, m.logo) };
        primary == self.command && !other && m.shift == self.shift && m.alt == self.alt
    }

    /// Keys a focused text field handles itself, which therefore must not also
    /// trigger an app shortcut while the user is typing.
    pub(crate) fn is_text_editing(&self) -> bool {
        match self.key {
            Key::Backspace
            | Key::Delete
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End
            | Key::Enter
            | Key::Escape
            | Key::Tab => true,
            // Only as shortcuts: plain C is typing, Cmd+C is copy.
            Key::A | Key::C | Key::X | Key::V => self.command,
            _ => false,
        }
    }

    /// How this shortcut should be written in a menu: `⌘⇧S` on Apple
    /// platforms, `Ctrl+Shift+S` elsewhere.
    pub fn label(&self, mac: bool) -> String {
        let name = self.key.label();
        if mac {
            let mut s = String::new();
            if self.command {
                s.push('\u{2318}');
            }
            if self.alt {
                s.push('\u{2325}');
            }
            if self.shift {
                s.push('\u{21E7}');
            }
            s + name
        } else {
            let mut parts: Vec<&str> = Vec::new();
            if self.command {
                parts.push("Ctrl");
            }
            if self.alt {
                parts.push("Alt");
            }
            if self.shift {
                parts.push("Shift");
            }
            parts.push(name);
            parts.join("+")
        }
    }
}

impl Key {
    /// Short display name, for menus and keymap editors.
    pub fn label(self) -> &'static str {
        use Key::*;
        match self {
            Enter => "Enter",
            Escape => "Esc",
            Backspace => "Backspace",
            Tab => "Tab",
            Space => "Space",
            Delete => "Del",
            Home => "Home",
            End => "End",
            PageUp => "PgUp",
            PageDown => "PgDn",
            Insert => "Ins",
            ArrowLeft => "Left",
            ArrowRight => "Right",
            ArrowUp => "Up",
            ArrowDown => "Down",
            Minus => "-",
            Equal => "=",
            BracketLeft => "[",
            BracketRight => "]",
            Backslash => "\\",
            Semicolon => ";",
            Quote => "'",
            Backquote => "`",
            Comma => ",",
            Period => ".",
            Slash => "/",
            A => "A",
            B => "B",
            C => "C",
            D => "D",
            E => "E",
            F => "F",
            G => "G",
            H => "H",
            I => "I",
            J => "J",
            K => "K",
            L => "L",
            M => "M",
            N => "N",
            O => "O",
            P => "P",
            Q => "Q",
            R => "R",
            S => "S",
            T => "T",
            U => "U",
            V => "V",
            W => "W",
            X => "X",
            Y => "Y",
            Z => "Z",
            Num0 => "0",
            Num1 => "1",
            Num2 => "2",
            Num3 => "3",
            Num4 => "4",
            Num5 => "5",
            Num6 => "6",
            Num7 => "7",
            Num8 => "8",
            Num9 => "9",
            F1 => "F1",
            F2 => "F2",
            F3 => "F3",
            F4 => "F4",
            F5 => "F5",
            F6 => "F6",
            F7 => "F7",
            F8 => "F8",
            F9 => "F9",
            F10 => "F10",
            F11 => "F11",
            F12 => "F12",
            F13 => "F13",
            F14 => "F14",
            F15 => "F15",
            F16 => "F16",
            F17 => "F17",
            F18 => "F18",
            F19 => "F19",
            F20 => "F20",
            F21 => "F21",
            F22 => "F22",
            F23 => "F23",
            F24 => "F24",
            Numpad0 => "Num0",
            Numpad1 => "Num1",
            Numpad2 => "Num2",
            Numpad3 => "Num3",
            Numpad4 => "Num4",
            Numpad5 => "Num5",
            Numpad6 => "Num6",
            Numpad7 => "Num7",
            Numpad8 => "Num8",
            Numpad9 => "Num9",
            NumpadDivide => "Num/",
            NumpadMultiply => "Num*",
            NumpadSubtract => "Num-",
            NumpadAdd => "Num+",
            NumpadEnter => "NumEnter",
            NumpadDecimal => "Num.",
            NumLock => "NumLock",
            CapsLock => "CapsLock",
            PrintScreen => "PrtSc",
            ScrollLock => "ScrLk",
            Pause => "Pause",
            ContextMenu => "Menu",
            ControlLeft | ControlRight => "Ctrl",
            ShiftLeft | ShiftRight => "Shift",
            AltLeft | AltRight => "Alt",
            SuperLeft | SuperRight => "Super",
        }
    }
}

/// (HID usage on page 0x07, key), in usage order.
const HID: &[(u16, Key)] = {
    use Key::*;
    &[
        (0x04, A), (0x05, B), (0x06, C), (0x07, D), (0x08, E), (0x09, F), (0x0A, G), (0x0B, H), (0x0C, I),
        (0x0D, J), (0x0E, K), (0x0F, L), (0x10, M), (0x11, N), (0x12, O), (0x13, P), (0x14, Q), (0x15, R),
        (0x16, S), (0x17, T), (0x18, U), (0x19, V), (0x1A, W), (0x1B, X), (0x1C, Y), (0x1D, Z),
        (0x1E, Num1), (0x1F, Num2), (0x20, Num3), (0x21, Num4), (0x22, Num5), (0x23, Num6), (0x24, Num7),
        (0x25, Num8), (0x26, Num9), (0x27, Num0),
        (0x28, Enter), (0x29, Escape), (0x2A, Backspace), (0x2B, Tab), (0x2C, Space),
        (0x2D, Minus), (0x2E, Equal), (0x2F, BracketLeft), (0x30, BracketRight), (0x31, Backslash),
        (0x33, Semicolon), (0x34, Quote), (0x35, Backquote), (0x36, Comma), (0x37, Period), (0x38, Slash),
        (0x39, CapsLock),
        (0x3A, F1), (0x3B, F2), (0x3C, F3), (0x3D, F4), (0x3E, F5), (0x3F, F6), (0x40, F7), (0x41, F8),
        (0x42, F9), (0x43, F10), (0x44, F11), (0x45, F12),
        (0x46, PrintScreen), (0x47, ScrollLock), (0x48, Pause), (0x49, Insert), (0x4A, Home), (0x4B, PageUp),
        (0x4C, Delete), (0x4D, End), (0x4E, PageDown), (0x4F, ArrowRight), (0x50, ArrowLeft), (0x51, ArrowDown),
        (0x52, ArrowUp),
        (0x53, NumLock), (0x54, NumpadDivide), (0x55, NumpadMultiply), (0x56, NumpadSubtract), (0x57, NumpadAdd),
        (0x58, NumpadEnter), (0x59, Numpad1), (0x5A, Numpad2), (0x5B, Numpad3), (0x5C, Numpad4), (0x5D, Numpad5),
        (0x5E, Numpad6), (0x5F, Numpad7), (0x60, Numpad8), (0x61, Numpad9), (0x62, Numpad0), (0x63, NumpadDecimal),
        (0x65, ContextMenu),
        (0x68, F13), (0x69, F14), (0x6A, F15), (0x6B, F16), (0x6C, F17), (0x6D, F18), (0x6E, F19), (0x6F, F20),
        (0x70, F21), (0x71, F22), (0x72, F23), (0x73, F24),
        (0xE0, ControlLeft), (0xE1, ShiftLeft), (0xE2, AltLeft), (0xE3, SuperLeft),
        (0xE4, ControlRight), (0xE5, ShiftRight), (0xE6, AltRight), (0xE7, SuperRight),
    ]
};

impl Key {
    /// Key for a USB HID keyboard usage (usage page 0x07), e.g. from a raw HID
    /// report. `None` for usages libgui has no key for.
    pub fn from_hid_usage(usage: u16) -> Option<Key> {
        HID.binary_search_by_key(&usage, |&(u, _)| u).ok().map(|i| HID[i].1)
    }

    /// Inverse of [`Key::from_hid_usage`].
    pub fn to_hid_usage(self) -> u16 {
        HID.iter().find(|&&(_, k)| k == self).map_or(0, |&(u, _)| u)
    }

    pub fn is_modifier(self) -> bool {
        use Key::*;
        matches!(self, ControlLeft | ControlRight | ShiftLeft | ShiftRight | AltLeft | AltRight | SuperLeft | SuperRight)
    }
}

/// Keyboard/text/clipboard events delivered to widgets this frame (derived from
/// `InputEvent`s: presses and repeats only, shortcuts already resolved).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum UiEvent {
    Key(Key, Modifiers),
    Text(String),
    Paste(String),
    Copy,
    Cut,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerKind {
    #[default]
    Mouse,
    Touch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Touch {
    pub id: u64,
    /// Logical px.
    pub pos: Vec2,
}

/// This frame's input, derived from the pushed events. Read it from custom
/// widgets via [`crate::Ui::input`].
#[derive(Clone, Debug)]
pub struct FrameInput {
    pub screen_size: Vec2,
    pub scale: f32,
    pub dt: f32,
    /// Primary pointer (mouse, or the first finger).
    pub mouse_pos: Vec2,
    pub mouse_inside: bool,
    /// Primary button (or one finger) held this frame.
    pub mouse_down: bool,
    pub buttons_down: [bool; 5],
    pub buttons_pressed: [bool; 5],
    /// Scroll since last frame, logical px.
    pub scroll: Vec2,
    /// The part of `scroll` that arrived in pixels (trackpads, precise
    /// wheels). It is already smooth, often with the OS's own momentum, so
    /// scroll areas follow it exactly; the rest (wheel notches) is eased.
    pub scroll_precise: Vec2,
    /// Sum of `PointerDelta`s since last frame, if any arrived.
    pub raw_delta: Option<Vec2>,
    pub modifiers: Modifiers,
    pub pointer_kind: PointerKind,
    /// Fingers down this frame, in landing order.
    pub touches: Vec<Touch>,
    /// Keys held (physical).
    pub keys_down: Vec<Key>,
    /// Keys that went down this frame (not repeats).
    pub keys_pressed: Vec<Key>,
    pub(crate) events: Vec<UiEvent>,
}

impl Default for FrameInput {
    fn default() -> Self {
        let info = FrameInfo::default();
        Self {
            screen_size: info.screen_size,
            scale: info.scale,
            dt: info.dt,
            mouse_pos: Vec2::ZERO,
            mouse_inside: false,
            mouse_down: false,
            buttons_down: [false; 5],
            buttons_pressed: [false; 5],
            scroll: Vec2::ZERO,
            scroll_precise: Vec2::ZERO,
            raw_delta: None,
            modifiers: Modifiers::default(),
            pointer_kind: PointerKind::Mouse,
            touches: Vec::new(),
            keys_down: Vec::new(),
            keys_pressed: Vec::new(),
            events: Vec::new(),
        }
    }
}

/// Turns relative motion (raw HID, locked mice) into an absolute pointer with
/// your own sensitivity and no OS acceleration. Feed the result to
/// `InputEvent::PointerMoved`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualCursor {
    pub pos: Vec2,
    /// Logical px per raw count.
    pub sensitivity: f32,
    /// The cursor is kept inside this rect (usually the window).
    pub bounds: Rect,
}

impl VirtualCursor {
    pub fn new(bounds: Rect, sensitivity: f32) -> Self {
        Self { pos: bounds.center(), sensitivity, bounds }
    }

    /// Move by a raw delta; returns the new position.
    pub fn apply(&mut self, delta: Vec2) -> Vec2 {
        let b = self.bounds;
        self.pos.x = (self.pos.x + delta.x * self.sensitivity).clamp(b.x, b.right() - 1.0);
        self.pos.y = (self.pos.y + delta.y * self.sensitivity).clamp(b.y, b.bottom() - 1.0);
        self.pos
    }
}

/// Two-finger gesture for this frame (touch only).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gesture {
    pub active: bool,
    pub center: Vec2,
    /// Movement of the two-finger centre since last frame.
    pub pan: Vec2,
    /// Distance ratio since last frame (1.0 = no zoom).
    pub zoom: f32,
}

impl Default for Gesture {
    fn default() -> Self {
        Self { active: false, center: Vec2::ZERO, pan: Vec2::ZERO, zoom: 1.0 }
    }
}

/// Cursor the UI would like the host to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Default,
    Pointer,
    ResizeHorizontal,
    ResizeVertical,
    /// Corner grips (south-east / north-west).
    ResizeDiagonal,
    Grab,
    Grabbing,
    Text,
}

/// Everything the UI asks of the host after a frame. One struct, so a host
/// can't miss a request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlatformOutput {
    pub cursor: Cursor,
    /// Text copied or cut this frame: put it on the OS clipboard.
    pub copied_text: Option<String>,
    /// Cmd/Ctrl+V was pressed in a text field: read the clipboard and push
    /// `InputEvent::Paste` (it applies on the next frame).
    pub paste_requested: bool,
    /// A text field has focus: show the on-screen keyboard / enable IME, and
    /// place the IME candidate window at this caret rect (logical px).
    pub text_input: Option<Rect>,
    /// The pointer is over, or dragging, UI: don't route it to the game.
    pub wants_pointer: bool,
    /// A text field has focus: don't route keys to the game.
    pub wants_keyboard: bool,
    /// A widget wants relative mode (e.g. an endless viewport orbit): hide and
    /// lock the cursor, and send `InputEvent::PointerDelta`.
    pub pointer_lock: bool,
    /// When the UI needs the next frame: `Some(0.0)` = as soon as possible
    /// (animating, dragging), `Some(t)` = in `t` seconds (caret blink), `None`
    /// = only after new input. Idle tools can sleep instead of redrawing.
    pub repaint_after: Option<f32>,
}
