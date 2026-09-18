use crate::Vec2;

/// Per-frame input snapshot, supplied by the host (winit, SDL, your engine…).
/// Positions are in logical pixels.
#[derive(Clone, Debug)]
pub struct Input {
    pub screen_size: Vec2,
    pub scale: f32,
    pub dt: f32,
    pub mouse_pos: Vec2,
    pub mouse_inside: bool,
    pub mouse_down: bool,
    /// Scroll delta accumulated since last frame, in logical pixels.
    pub scroll: Vec2,
    pub modifiers: Modifiers,
    /// Keyboard / text / clipboard events since last frame, in order.
    pub events: Vec<Event>,
    /// Mouse or touch. With `Touch`, fill `touches`; libgui derives the
    /// primary pointer (`mouse_pos`/`mouse_down`) and gestures from them.
    pub pointer_kind: PointerKind,
    /// Fingers currently on the screen, in the order they landed.
    pub touches: Vec<Touch>,
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

impl Default for Input {
    fn default() -> Self {
        Self {
            screen_size: Vec2::new(1280.0, 800.0),
            scale: 1.0,
            dt: 1.0 / 60.0,
            mouse_pos: Vec2::ZERO,
            mouse_inside: false,
            mouse_down: false,
            scroll: Vec2::ZERO,
            modifiers: Modifiers::default(),
            events: Vec::new(),
            pointer_kind: PointerKind::Mouse,
            touches: Vec::new(),
        }
    }
}

/// Platform-neutral modifiers. The host decides what `command` and `word`
/// mean: on macOS `command` = Cmd and `word` = Option; elsewhere both are Ctrl.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    /// Shortcut modifier (select all, line start/end).
    pub command: bool,
    /// Word-wise caret movement / deletion.
    pub word: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Escape,
    Tab,
    /// Only needed with `command` (select all).
    A,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Key press (including OS key-repeat).
    Key(Key, Modifiers),
    /// Committed text: typed characters or IME commit.
    Text(String),
    /// Host read the clipboard in response to Cmd/Ctrl+V.
    Paste(String),
    /// Cmd/Ctrl+C: libgui puts the selection in `Ui::take_copied`.
    Copy,
    /// Cmd/Ctrl+X.
    Cut,
}

/// Cursor the UI would like the host to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Default,
    Pointer,
    ResizeHorizontal,
    ResizeVertical,
    /// Top-left <-> bottom-right, for corner resize grips.
    ResizeDiagonal,
    Grab,
    Grabbing,
    Text,
}
