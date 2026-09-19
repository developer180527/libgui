//! Turns the event stream into one `FrameInput` per frame.
//!
//! This is where input *timing* is decided, once, for every host:
//! - a press and release inside one frame is seen as pressed for one frame,
//!   then released (quick taps and trackpad clicks are never lost);
//! - fingers that land and lift inside one frame likewise;
//! - modifiers follow `ModifiersChanged`, or are derived from modifier `Key`
//!   events (raw HID hosts send only keys);
//! - Cmd/Ctrl+C/X/V become copy/cut/paste requests, and text typed while a
//!   shortcut modifier is held is not inserted.

use crate::input::UiEvent;
use crate::{FrameInfo, FrameInput, InputEvent, Key, Modifiers, PointerKind, Touch, TouchPhase, Vec2, WheelUnit};

/// Logical px per wheel line / page.
const LINE: f32 = 24.0;
const PAGE: f32 = 480.0;

#[derive(Clone, Copy, Default)]
struct Button {
    down: bool,
    /// Went down since the last frame.
    pressed: bool,
    /// Released, but only after the next frame (it was pressed within this one).
    release_after_frame: bool,
}

struct Finger {
    id: u64,
    pos: Vec2,
    fresh: bool,
    lifted: bool,
}

pub(crate) struct InputState {
    queue: Vec<InputEvent>,
    pos: Vec2,
    inside: bool,
    kind: PointerKind,
    buttons: [Button; 5],
    fingers: Vec<Finger>,
    keys_down: Vec<Key>,
    mods: Modifiers,
    /// Apple-style shortcuts (Cmd) vs Ctrl.
    pub mac: bool,
    pub paste_requested: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            pos: Vec2::ZERO,
            inside: false,
            kind: PointerKind::Mouse,
            buttons: [Button::default(); 5],
            fingers: Vec::new(),
            keys_down: Vec::new(),
            mods: Modifiers::default(),
            mac: cfg!(any(target_os = "macos", target_os = "ios")),
            paste_requested: false,
        }
    }

    pub fn push(&mut self, e: InputEvent) {
        self.queue.push(e);
    }

    fn derive_mods(&mut self) {
        let k = |a: Key, b: Key| self.keys_down.contains(&a) || self.keys_down.contains(&b);
        self.mods = Modifiers::from_keys(
            k(Key::ShiftLeft, Key::ShiftRight),
            k(Key::ControlLeft, Key::ControlRight),
            k(Key::AltLeft, Key::AltRight),
            k(Key::SuperLeft, Key::SuperRight),
            self.mac,
        );
    }

    /// Consume the queued events and produce this frame's input.
    pub fn frame(&mut self, info: FrameInfo) -> FrameInput {
        let mut out = FrameInput { screen_size: info.screen_size, scale: info.scale, dt: info.dt, ..FrameInput::default() };
        self.paste_requested = false;
        for e in std::mem::take(&mut self.queue) {
            match e {
                InputEvent::PointerMoved { pos } => {
                    self.pos = pos;
                    self.inside = true;
                    if self.fingers.is_empty() {
                        self.kind = PointerKind::Mouse;
                    }
                }
                InputEvent::PointerDelta { delta } => {
                    out.raw_delta = Some(out.raw_delta.unwrap_or(Vec2::ZERO) + delta);
                }
                InputEvent::PointerLeft => self.inside = false,
                InputEvent::PointerButton { button, pressed } => {
                    let b = &mut self.buttons[button.index()];
                    if pressed {
                        b.down = true;
                        b.pressed = true;
                        b.release_after_frame = false;
                    } else if b.pressed {
                        b.release_after_frame = true;
                    } else {
                        b.down = false;
                    }
                    self.kind = PointerKind::Mouse;
                }
                InputEvent::Wheel { delta, unit } => {
                    let k = match unit {
                        WheelUnit::Pixel => 1.0,
                        WheelUnit::Line => LINE,
                        WheelUnit::Page => PAGE,
                    };
                    out.scroll += delta * k;
                    if unit == WheelUnit::Pixel {
                        out.scroll_precise += delta;
                    }
                }
                InputEvent::Touch { id, phase, pos } => {
                    self.kind = PointerKind::Touch;
                    match phase {
                        TouchPhase::Start => self.fingers.push(Finger { id, pos, fresh: true, lifted: false }),
                        TouchPhase::Move => {
                            if let Some(f) = self.fingers.iter_mut().find(|f| f.id == id) {
                                f.pos = pos;
                            }
                        }
                        TouchPhase::End => {
                            if let Some(f) = self.fingers.iter_mut().find(|f| f.id == id) {
                                f.pos = pos;
                                f.lifted = true;
                            }
                            // Already seen by a frame: release now; else after one frame.
                            self.fingers.retain(|f| !f.lifted || f.fresh);
                        }
                        TouchPhase::Cancel => self.fingers.retain(|f| f.id != id),
                    }
                }
                InputEvent::Key { key, pressed, repeat } => {
                    if pressed {
                        if !self.keys_down.contains(&key) {
                            self.keys_down.push(key);
                        }
                    } else {
                        self.keys_down.retain(|&k| k != key);
                    }
                    if key.is_modifier() {
                        self.derive_mods();
                        continue;
                    }
                    if !pressed {
                        continue;
                    }
                    if !repeat {
                        out.keys_pressed.push(key);
                    }
                    let m = self.mods;
                    match key {
                        Key::C if m.command => out.events.push(UiEvent::Copy),
                        Key::X if m.command => out.events.push(UiEvent::Cut),
                        Key::V if m.command => self.paste_requested = true,
                        _ => out.events.push(UiEvent::Key(key, m)),
                    }
                }
                InputEvent::ModifiersChanged(m) => {
                    self.mods = Modifiers::from_keys(m.shift, m.ctrl, m.alt, m.logo, self.mac);
                }
                InputEvent::Text(s) => {
                    // Shortcut chords (Cmd+S) aren't text; AltGr (Ctrl+Alt) still is.
                    if self.mods.command && !self.mods.alt {
                        continue;
                    }
                    let s: String = s.chars().filter(|c| !c.is_control()).collect();
                    if !s.is_empty() {
                        out.events.push(UiEvent::Text(s));
                    }
                }
                InputEvent::Paste(s) => out.events.push(UiEvent::Paste(s)),
                InputEvent::Copy => out.events.push(UiEvent::Copy),
                InputEvent::Cut => out.events.push(UiEvent::Cut),
                InputEvent::FocusLost => {
                    for b in &mut self.buttons {
                        b.release_after_frame = b.pressed;
                        b.down &= b.pressed;
                    }
                    self.keys_down.clear();
                    self.fingers.clear();
                    self.derive_mods();
                }
            }
        }

        out.pointer_kind = self.kind;
        out.mouse_pos = self.fingers.first().map_or(self.pos, |f| f.pos);
        out.mouse_inside = self.inside;
        for (i, b) in self.buttons.iter().enumerate() {
            out.buttons_down[i] = b.down;
            out.buttons_pressed[i] = b.pressed;
        }
        out.mouse_down = out.buttons_down[0];
        out.touches = self.fingers.iter().map(|f| Touch { id: f.id, pos: f.pos }).collect();
        out.keys_down = self.keys_down.clone();
        out.modifiers = self.mods;

        // This frame has seen the presses: apply deferred releases for the next.
        for b in &mut self.buttons {
            if b.release_after_frame {
                b.down = false;
                b.release_after_frame = false;
            }
            b.pressed = false;
        }
        self.fingers.retain(|f| !f.lifted);
        for f in &mut self.fingers {
            f.fresh = false;
        }
        if let Some(f) = self.fingers.first() {
            self.pos = f.pos;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::{FrameInfo, InputEvent as IE, Key, PointerButton, Rect, Response, Theme, Ui, Vec2, VirtualCursor};

    fn ui() -> Ui {
        Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).unwrap()
    }

    fn frame(ui: &mut Ui, events: Vec<IE>, build: &mut dyn FnMut(&mut Ui) -> Response) -> (Response, crate::PlatformOutput) {
        for e in events {
            ui.push(e);
        }
        ui.begin_frame(FrameInfo::default());
        let r = build(ui);
        let out = ui.end_frame().platform;
        (r, out)
    }

    fn btn(button: PointerButton, pressed: bool) -> IE {
        IE::PointerButton { button, pressed }
    }

    #[test]
    fn click_shorter_than_a_frame_still_clicks_once() {
        let mut ui = ui();
        let mut b = |ui: &mut Ui| ui.button("OK");
        frame(&mut ui, vec![IE::PointerMoved { pos: Vec2::new(10.0, 10.0) }], &mut b);
        let (r, _) = frame(&mut ui, vec![btn(PointerButton::Primary, true), btn(PointerButton::Primary, false)], &mut b);
        assert!(r.pressed && !r.clicked, "the frame sees the press");
        let (r, _) = frame(&mut ui, vec![], &mut b);
        assert!(r.clicked, "and the next one the release");
        let (r, _) = frame(&mut ui, vec![], &mut b);
        assert!(!r.clicked && !r.active, "exactly once");
    }

    #[test]
    fn hid_usages_map_to_keys_and_back() {
        assert_eq!(Key::from_hid_usage(0x04), Some(Key::A));
        assert_eq!(Key::from_hid_usage(0x27), Some(Key::Num0));
        assert_eq!(Key::from_hid_usage(0x28), Some(Key::Enter));
        assert_eq!(Key::from_hid_usage(0x52), Some(Key::ArrowUp));
        assert_eq!(Key::from_hid_usage(0xE3), Some(Key::SuperLeft));
        assert_eq!(Key::from_hid_usage(0x32), None, "non-US # is not mapped");
        for usage in 0u16..=0xFF {
            if let Some(k) = Key::from_hid_usage(usage) {
                assert_eq!(k.to_hid_usage(), usage);
            }
        }
    }

    #[test]
    fn modifiers_come_from_raw_modifier_keys() {
        let mut ui = ui();
        ui.set_mac_shortcuts(false);
        let key = |key, pressed| IE::Key { key, pressed, repeat: false };
        let mut nothing = |ui: &mut Ui| ui.interact(crate::Id::new("x"));
        frame(&mut ui, vec![key(Key::ControlLeft, true), key(Key::ShiftRight, true)], &mut nothing);
        let m = ui.input().modifiers;
        assert!(m.ctrl && m.shift && m.command && m.word && !m.alt);
        frame(&mut ui, vec![key(Key::ControlLeft, false)], &mut nothing);
        let m = ui.input().modifiers;
        assert!(!m.command && m.shift);
        frame(&mut ui, vec![IE::FocusLost], &mut nothing);
        assert_eq!(ui.input().modifiers, Default::default(), "focus loss releases everything");
        assert!(ui.input().keys_down.is_empty());
    }

    #[test]
    fn raw_deltas_drive_drags_only_while_locked() {
        let mut ui = ui();
        let id = crate::Id::new("vp");
        let mut vp = |ui: &mut Ui| {
            let r = ui.interact_drag(id);
            if r.active {
                ui.request_pointer_lock();
            }
            ui.add_leaf(id, crate::Layout::leaf(crate::Size::Fixed(200.0), crate::Size::Fixed(200.0)), Vec2::ZERO, true, |_, _| {});
            r
        };
        frame(&mut ui, vec![IE::PointerMoved { pos: Vec2::new(50.0, 50.0) }], &mut vp);
        let (_, out) = frame(&mut ui, vec![btn(PointerButton::Primary, true)], &mut vp);
        assert!(out.pointer_lock, "an active viewport asks for relative mode");

        // Locked: the (unaccelerated) raw delta wins over the cursor movement.
        let (r, _) = frame(
            &mut ui,
            vec![IE::PointerMoved { pos: Vec2::new(60.0, 50.0) }, IE::PointerDelta { delta: Vec2::new(3.0, -1.0) }],
            &mut vp,
        );
        assert_eq!(r.drag_delta, Vec2::new(3.0, -1.0));
        assert_eq!(r.raw_delta, Some(Vec2::new(3.0, -1.0)));

        // Released: no lock, and a splitter-style drag follows the cursor again.
        let (_, out) = frame(&mut ui, vec![btn(PointerButton::Primary, false)], &mut vp);
        assert!(!out.pointer_lock);
    }

    #[test]
    fn secondary_button_and_idle_repaint_hint() {
        let mut ui = ui();
        let mut b = |ui: &mut Ui| ui.button("Menu");
        frame(&mut ui, vec![IE::PointerMoved { pos: Vec2::new(10.0, 10.0) }], &mut b);
        let (r, _) = frame(&mut ui, vec![btn(PointerButton::Secondary, true)], &mut b);
        assert!(r.secondary_pressed && !r.pressed, "right-click doesn't press the button");
        frame(&mut ui, vec![btn(PointerButton::Secondary, false)], &mut b);

        // Hovering starts an animation: repaint soon. Once settled: sleep.
        let (_, out) = frame(&mut ui, vec![], &mut b);
        assert_eq!(out.repaint_after, Some(0.0));
        let mut out = out;
        for _ in 0..120 {
            out = frame(&mut ui, vec![], &mut b).1;
        }
        assert_eq!(out.repaint_after, None, "an idle UI lets the host sleep");
        assert!(out.wants_pointer, "still hovering UI");
    }

    #[test]
    fn virtual_cursor_accumulates_and_clamps() {
        let mut c = VirtualCursor::new(Rect::new(0.0, 0.0, 100.0, 50.0), 0.5);
        assert_eq!(c.pos, Vec2::new(50.0, 25.0));
        assert_eq!(c.apply(Vec2::new(10.0, 4.0)), Vec2::new(55.0, 27.0));
        assert_eq!(c.apply(Vec2::new(1000.0, -1000.0)), Vec2::new(99.0, 0.0));
    }
}
