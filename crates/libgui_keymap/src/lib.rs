//! Cross-platform keymaps for libgui.
//!
//! libgui itself has no keymap: its widgets act on [`UiAction`]s, app
//! shortcuts are concrete [`Shortcut`] chords, and nothing in it knows which
//! OS it runs on. This crate is where platform conventions live:
//!
//! - **libgui's own actions**, with each platform's native bindings:
//!   Option-word and Cmd-line movement plus the Emacs keys on macOS,
//!   Ctrl-word and Ctrl+Insert/Shift+Insert on Windows and Linux.
//! - **Your app's actions**, declared once with platform-neutral [`Chord`]s
//!   (`Chord::primary(Key::S)` is Cmd+S on a Mac and Ctrl+S elsewhere) and
//!   rebindable at runtime.
//! - **Spelling** chords for menus the way each platform does: `⇧⌘S`,
//!   `Ctrl+Shift+S`.
//!
//! ```ignore
//! #[derive(Clone, Copy, PartialEq)]
//! enum Action { Save, Undo, Redo, Frame }
//!
//! let mut keys = Keymap::for_current_platform();
//! keys.bind(Chord::primary(Key::S), Action::Save)
//!     .bind(Chord::primary(Key::Z), Action::Undo)
//!     .bind(Chord::primary(Key::Z).shift(), Action::Redo)
//!     .bind(Key::F, Action::Frame);
//! keys.install(&mut ui); // libgui's text fields, focus and popups
//!
//! // Each frame:
//! if keys.triggered(&mut ui, Action::Save) { save(); }
//! if ui.menu_item_shortcut("Save", &keys.label(Action::Save)).clicked { save(); }
//! ```
//!
//! Hosts that resolve keys themselves (a game engine's own input layer, raw
//! HID) can skip this crate: build a [`KeyBindings`] by hand, or push
//! [`libgui::InputEvent::Action`]s directly.

use libgui::{Key, KeyBindings, Modifiers, Motion, Nav, Shortcut, Ui, UiAction};

/// Whose conventions to follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    /// macOS and iPadOS (hardware keyboards).
    Mac,
    Windows,
    /// Linux, the BSDs, and anything else X11/Wayland-like.
    Linux,
}

impl Platform {
    /// The platform this was compiled for. The only place in libgui's crates
    /// that asks; pass a platform explicitly to follow another's conventions
    /// (a Mac user on a Linux VM, tests, a user preference).
    pub fn current() -> Self {
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            Platform::Mac
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// A key chord written in terms of what the modifiers *mean*, resolved to a
/// concrete [`Shortcut`] per platform.
///
/// `primary` is the platform's command key: Cmd on a Mac, Ctrl elsewhere.
/// `ctrl`, `alt`, `shift` and `logo` are the literal keys on every platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub primary: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

impl Chord {
    /// A bare key.
    pub const fn key(key: Key) -> Self {
        Self { key, primary: false, ctrl: false, alt: false, shift: false, logo: false }
    }

    /// Cmd+key on a Mac, Ctrl+key elsewhere.
    pub const fn primary(key: Key) -> Self {
        Self { primary: true, ..Self::key(key) }
    }

    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Alt; Option on a Mac.
    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// The literal Control key, on every platform (on a Mac, the ⌃ key).
    pub const fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// The literal Cmd / Windows / Super key.
    pub const fn logo(mut self) -> Self {
        self.logo = true;
        self
    }

    /// The concrete chord on `platform`.
    pub fn resolve(&self, platform: Platform) -> Shortcut {
        let mac = platform == Platform::Mac;
        Shortcut::new(
            self.key,
            Modifiers {
                shift: self.shift,
                alt: self.alt,
                ctrl: self.ctrl || (self.primary && !mac),
                logo: self.logo || (self.primary && mac),
            },
        )
    }
}

impl From<Key> for Chord {
    fn from(key: Key) -> Self {
        Chord::key(key)
    }
}

/// The platform's native bindings for libgui's widget actions: caret
/// movement and selection, deletion, clipboard, Submit/Cancel and focus
/// traversal. [`Keymap`] starts from these; call this directly to install
/// them without app actions.
pub fn ui_bindings(platform: Platform) -> KeyBindings {
    let mut b = KeyBindings::new();
    let p = |c: Chord| c.resolve(platform);
    // A movement, and the same chord with Shift extending the selection.
    let mv = |b: &mut KeyBindings, c: Chord, motion: Motion| {
        b.bind(p(c), UiAction::Move { motion, select: false });
        b.bind(p(c.shift()), UiAction::Move { motion, select: true });
    };
    use Key::*;
    use Motion::*;

    // Everywhere.
    mv(&mut b, Chord::key(ArrowLeft), Left);
    mv(&mut b, Chord::key(ArrowRight), Right);
    mv(&mut b, Chord::key(ArrowUp), Up);
    mv(&mut b, Chord::key(ArrowDown), Down);
    mv(&mut b, Chord::key(Home), LineStart);
    mv(&mut b, Chord::key(End), LineEnd);
    let del = |b: &mut KeyBindings, c: Chord, m: Motion| {
        b.bind(p(c), UiAction::Delete(m));
    };
    del(&mut b, Chord::key(Backspace), Left);
    // Shift held while correcting a capital still deletes, as natively.
    del(&mut b, Chord::key(Backspace).shift(), Left);
    del(&mut b, Chord::key(Delete), Right);
    let act = |b: &mut KeyBindings, c: Chord, a: UiAction| {
        b.bind(p(c), a);
    };
    act(&mut b, Chord::primary(A), UiAction::SelectAll);
    act(&mut b, Chord::primary(C), UiAction::Copy);
    act(&mut b, Chord::primary(X), UiAction::Cut);
    act(&mut b, Chord::primary(V), UiAction::Paste);
    // Enter breaks a line; a single-line field has nowhere to put one and
    // commits instead, so one binding serves both.
    act(&mut b, Chord::key(Enter), UiAction::InsertNewline);
    act(&mut b, Chord::key(NumpadEnter), UiAction::InsertNewline);
    act(&mut b, Chord::primary(Enter), UiAction::Submit);
    // The field's own undo, not the app's: it only reaches a focused field.
    act(&mut b, Chord::primary(Z), UiAction::Undo);
    act(&mut b, Chord::primary(Z).shift(), UiAction::Redo);
    act(&mut b, Chord::key(Escape), UiAction::Cancel);
    act(&mut b, Chord::key(Tab), UiAction::FocusNext);
    act(&mut b, Chord::key(Tab).shift(), UiAction::FocusPrevious);

    // The arrows drive a list, tree or table cursor as well as a caret. The
    // same chord is deliberately bound twice: only one of a text field and a
    // collection can have focus, so each takes the action it understands and
    // the other is dropped. See `KeyBindings::bind`.
    let nav = |b: &mut KeyBindings, c: Chord, n: Nav| {
        b.bind(p(c), UiAction::Navigate(n));
    };
    nav(&mut b, Chord::key(ArrowDown), Nav::Next);
    nav(&mut b, Chord::key(ArrowUp), Nav::Previous);
    nav(&mut b, Chord::key(ArrowRight), Nav::Expand);
    nav(&mut b, Chord::key(ArrowLeft), Nav::Collapse);
    nav(&mut b, Chord::key(Home), Nav::First);
    nav(&mut b, Chord::key(End), Nav::Last);
    nav(&mut b, Chord::key(PageDown), Nav::PageNext);
    nav(&mut b, Chord::key(PageUp), Nav::PagePrevious);

    match platform {
        Platform::Mac => {
            // Option moves by word, Cmd to the ends.
            mv(&mut b, Chord::key(ArrowLeft).alt(), WordLeft);
            mv(&mut b, Chord::key(ArrowRight).alt(), WordRight);
            mv(&mut b, Chord::primary(ArrowLeft), LineStart);
            mv(&mut b, Chord::primary(ArrowRight), LineEnd);
            mv(&mut b, Chord::primary(ArrowUp), DocStart);
            mv(&mut b, Chord::primary(ArrowDown), DocEnd);
            del(&mut b, Chord::key(Backspace).alt(), WordLeft);
            del(&mut b, Chord::key(Delete).alt(), WordRight);
            del(&mut b, Chord::primary(Backspace), LineStart);
            del(&mut b, Chord::primary(Delete), LineEnd);
            // The Emacs keys every Cocoa text field honours.
            mv(&mut b, Chord::key(A).ctrl(), LineStart);
            mv(&mut b, Chord::key(E).ctrl(), LineEnd);
            mv(&mut b, Chord::key(B).ctrl(), Left);
            mv(&mut b, Chord::key(F).ctrl(), Right);
            mv(&mut b, Chord::key(P).ctrl(), Up);
            mv(&mut b, Chord::key(N).ctrl(), Down);
            del(&mut b, Chord::key(H).ctrl(), Left);
            del(&mut b, Chord::key(D).ctrl(), Right);
            del(&mut b, Chord::key(K).ctrl(), LineEnd);
        }
        Platform::Windows | Platform::Linux => {
            mv(&mut b, Chord::key(ArrowLeft).ctrl(), WordLeft);
            mv(&mut b, Chord::key(ArrowRight).ctrl(), WordRight);
            mv(&mut b, Chord::key(Home).ctrl(), DocStart);
            mv(&mut b, Chord::key(End).ctrl(), DocEnd);
            del(&mut b, Chord::key(Backspace).ctrl(), WordLeft);
            del(&mut b, Chord::key(Delete).ctrl(), WordRight);
            // Redo's second chord, which Windows has had since Word 2.0.
            act(&mut b, Chord::primary(Y), UiAction::Redo);
            // The IBM CUA clipboard keys, still standard on both.
            act(&mut b, Chord::key(Insert).ctrl(), UiAction::Copy);
            act(&mut b, Chord::key(Delete).shift(), UiAction::Cut);
            act(&mut b, Chord::key(Insert).shift(), UiAction::Paste);
        }
    }
    b
}

/// A platform's widget bindings plus the app's own actions.
///
/// `A` is the app's action type, usually a small `Copy` enum. Bindings are
/// resolved to concrete chords when added, so rebinding at runtime is just
/// [`Keymap::rebind`]; the UI table can be edited through
/// [`Keymap::ui_bindings_mut`] and re-[`install`](Keymap::install)ed.
#[derive(Clone, Debug)]
pub struct Keymap<A> {
    platform: Platform,
    ui: KeyBindings,
    app: Vec<(Shortcut, A)>,
}

impl<A: Copy + PartialEq> Keymap<A> {
    /// `platform`'s widget bindings and no app actions.
    pub fn new(platform: Platform) -> Self {
        Self { platform, ui: ui_bindings(platform), app: Vec::new() }
    }

    pub fn for_current_platform() -> Self {
        Self::new(Platform::current())
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Hand the widget bindings to `ui`. Call once per `Ui` (each window has
    /// one), and again after editing them.
    /// Install both halves of this platform's keyboard convention: the chords
    /// libgui's widgets act on, and where the keyboard is allowed to go.
    pub fn install(&self, ui: &mut Ui) {
        ui.set_key_bindings(self.ui.clone());
        ui.focus_policy = focus_policy(self.platform);
    }

    pub fn ui_bindings(&self) -> &KeyBindings {
        &self.ui
    }

    pub fn ui_bindings_mut(&mut self) -> &mut KeyBindings {
        &mut self.ui
    }

    /// Bind a platform-neutral chord to an app action. An action may have
    /// several chords; the first is the one menus show.
    pub fn bind(&mut self, chord: impl Into<Chord>, action: A) -> &mut Self {
        let sc = chord.into().resolve(self.platform);
        self.app.push((sc, action));
        self
    }

    /// Bind an exact chord, for a binding that only makes sense on one
    /// platform (Cmd+Q on a Mac, Alt+F4 on Windows).
    pub fn bind_exact(&mut self, shortcut: Shortcut, action: A) -> &mut Self {
        self.app.push((shortcut, action));
        self
    }

    /// Remove every chord bound to `action`.
    pub fn unbind(&mut self, action: A) -> &mut Self {
        self.app.retain(|(_, a)| *a != action);
        self
    }

    /// Replace `action`'s chords with `chord`: a keymap editor's "set".
    pub fn rebind(&mut self, action: A, chord: impl Into<Chord>) -> &mut Self {
        self.unbind(action);
        self.bind(chord, action)
    }

    /// The chords that trigger `action`, in binding order.
    pub fn shortcuts(&self, action: A) -> impl Iterator<Item = Shortcut> + '_ {
        self.app.iter().filter(move |(_, a)| *a == action).map(|&(s, _)| s)
    }

    /// The app action `shortcut` is bound to, if any: for a keymap editor
    /// warning about a clash.
    pub fn action_for(&self, shortcut: Shortcut) -> Option<A> {
        self.app.iter().find(|(s, _)| *s == shortcut).map(|&(_, a)| a)
    }

    /// True once per press of any chord bound to `action`, routed through
    /// [`Ui::consume_shortcut`]: focus, scopes, typing and one-shot
    /// consumption all apply.
    pub fn triggered(&self, ui: &mut Ui, action: A) -> bool {
        let mut hit = false;
        for sc in self.shortcuts(action) {
            if ui.consume_shortcut(sc) {
                hit = true;
                break;
            }
        }
        hit
    }

    /// `action`'s first chord spelled for this platform (`⇧⌘S`,
    /// `Ctrl+Shift+S`), or an empty string if it has none.
    pub fn label(&self, action: A) -> String {
        self.shortcuts(action).next().map(|s| format(s, self.platform)).unwrap_or_default()
    }

    /// Spell any chord for this keymap's platform.
    pub fn format(&self, shortcut: Shortcut) -> String {
        format(shortcut, self.platform)
    }
}

/// Spell a chord the way `platform` writes it in menus: Mac modifier
/// symbols in Apple's order with no separators (`⌃⌥⇧⌘S`); elsewhere words
/// joined with `+` (`Ctrl+Alt+Shift+S`).
pub fn format(shortcut: Shortcut, platform: Platform) -> String {
    let m = shortcut.mods;
    let key = key_name(shortcut.key, platform);
    match platform {
        Platform::Mac => {
            let mut s = String::new();
            for (on, sym) in [(m.ctrl, '⌃'), (m.alt, '⌥'), (m.shift, '⇧'), (m.logo, '⌘')] {
                if on {
                    s.push(sym);
                }
            }
            s + key
        }
        Platform::Windows | Platform::Linux => {
            let logo = if platform == Platform::Windows { "Win" } else { "Super" };
            let mut parts = Vec::new();
            for (on, name) in [(m.ctrl, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift"), (m.logo, logo)] {
                if on {
                    parts.push(name);
                }
            }
            parts.push(key);
            parts.join("+")
        }
    }
}

/// A key's name as `platform` writes it: `⌫` / `Backspace`, `←` / `Left`.
pub fn key_name(key: Key, platform: Platform) -> &'static str {
    use Key::*;
    if platform == Platform::Mac {
        let sym = match key {
            Enter => Some("↩"),
            NumpadEnter => Some("⌅"),
            Backspace => Some("⌫"),
            Delete => Some("⌦"),
            Escape => Some("⎋"),
            Tab => Some("⇥"),
            ArrowLeft => Some("←"),
            ArrowRight => Some("→"),
            ArrowUp => Some("↑"),
            ArrowDown => Some("↓"),
            Home => Some("↖"),
            End => Some("↘"),
            PageUp => Some("⇞"),
            PageDown => Some("⇟"),
            _ => None,
        };
        if let Some(s) = sym {
            return s;
        }
    }
    match key {
        Enter => "Enter",
        Escape => "Esc",
        Backspace => "Backspace",
        Tab => "Tab",
        Space => "Space",
        Delete => "Del",
        Insert => "Ins",
        Home => "Home",
        End => "End",
        PageUp => "PgUp",
        PageDown => "PgDn",
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

#[cfg(test)]
mod tests {
    use super::*;
    use libgui::{FrameInfo, InputEvent, Theme, Vec2};

    const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
    const ALL: [Platform; 3] = [Platform::Mac, Platform::Windows, Platform::Linux];

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Act {
        Save,
        Redo,
        Frame,
    }

    #[test]
    fn primary_is_cmd_on_mac_and_ctrl_elsewhere() {
        let s = Chord::primary(Key::S).shift();
        assert_eq!(s.resolve(Platform::Mac), Shortcut::plain(Key::S).logo().shift());
        assert_eq!(s.resolve(Platform::Windows), Shortcut::plain(Key::S).ctrl().shift());
        // The literal Control key stays Control on a Mac.
        assert_eq!(Chord::key(Key::A).ctrl().resolve(Platform::Mac), Shortcut::plain(Key::A).ctrl());
    }

    /// Which consumer would take an action: a focused text field, or a
    /// focused collection. One chord may serve both — Down moves a caret and
    /// moves a list cursor — because only one of them can have focus.
    fn consumer(a: UiAction) -> &'static str {
        match a {
            UiAction::Navigate(_) => "collection",
            _ => "widget",
        }
    }

    /// A chord bound twice **for the same consumer** would silently lose one
    /// of its actions: the focused widget takes the first and never sees the
    /// second. Binding one chord to a caret motion *and* a navigation is the
    /// deliberate case and is allowed.
    #[test]
    fn no_platform_binds_a_chord_twice_for_one_consumer() {
        for p in ALL {
            let b = ui_bindings(p);
            let seen: Vec<(Shortcut, &str)> = b.iter().map(|(s, a)| (s, consumer(a))).collect();
            for (i, c) in seen.iter().enumerate() {
                assert!(!seen[..i].contains(c), "{p:?}: {} bound twice for the {}", format(c.0, p), c.1);
            }
        }
    }

    /// The arrows reach a list on every platform, and still reach a caret.
    /// Losing either half is the kind of thing that only shows up when
    /// someone tries to use the app without a mouse.
    #[test]
    fn the_arrows_drive_both_a_caret_and_a_cursor() {
        for p in ALL {
            let b = ui_bindings(p);
            let plain = Modifiers::default();
            let all = |k: Key| -> Vec<UiAction> { b.resolve_all(k, &plain).collect() };
            for (key, nav, motion) in [
                (Key::ArrowDown, Nav::Next, Motion::Down),
                (Key::ArrowUp, Nav::Previous, Motion::Up),
                (Key::ArrowRight, Nav::Expand, Motion::Right),
                (Key::ArrowLeft, Nav::Collapse, Motion::Left),
            ] {
                let acts = all(key);
                assert!(acts.contains(&UiAction::Navigate(nav)), "{p:?}: {key:?} does not move a list cursor");
                assert!(
                    acts.contains(&UiAction::Move { motion, select: false }),
                    "{p:?}: {key:?} stopped moving a caret"
                );
            }
            assert!(all(Key::PageDown).contains(&UiAction::Navigate(Nav::PageNext)));
            assert!(all(Key::Home).contains(&UiAction::Navigate(Nav::First)));
            assert!(all(Key::End).contains(&UiAction::Navigate(Nav::Last)));
        }
    }

    /// Every action a text field or focus traversal needs is reachable on
    /// every platform.
    #[test]
    fn every_platform_covers_the_widget_actions() {
        let needed = [
            UiAction::Move { motion: Motion::Left, select: false },
            UiAction::Move { motion: Motion::WordRight, select: true },
            UiAction::Move { motion: Motion::LineStart, select: false },
            UiAction::Move { motion: Motion::DocEnd, select: true },
            UiAction::Delete(Motion::Left),
            UiAction::Delete(Motion::WordLeft),
            UiAction::Delete(Motion::WordRight),
            UiAction::SelectAll,
            UiAction::Copy,
            UiAction::Cut,
            UiAction::Paste,
            UiAction::Submit,
            UiAction::Cancel,
            UiAction::FocusNext,
            UiAction::FocusPrevious,
        ];
        for p in ALL {
            let b = ui_bindings(p);
            for a in needed {
                assert!(b.iter().any(|(_, x)| x == a), "{p:?} has no chord for {a:?}");
            }
        }
    }

    #[test]
    fn labels_follow_the_platform() {
        let mut k = Keymap::new(Platform::Mac);
        k.bind(Chord::primary(Key::S).shift(), Act::Save);
        assert_eq!(k.label(Act::Save), "⇧⌘S");
        assert_eq!(k.format(Shortcut::plain(Key::Backspace).alt()), "⌥⌫");
        assert_eq!(format(Shortcut::plain(Key::A).ctrl().alt().shift().logo(), Platform::Mac), "⌃⌥⇧⌘A");

        let mut k = Keymap::new(Platform::Windows);
        k.bind(Chord::primary(Key::S).shift(), Act::Save);
        assert_eq!(k.label(Act::Save), "Ctrl+Shift+S");
        assert_eq!(format(Shortcut::plain(Key::F4).alt(), Platform::Windows), "Alt+F4");
        assert_eq!(format(Shortcut::plain(Key::E).logo(), Platform::Linux), "Super+E");
        assert_eq!(k.label(Act::Frame), "", "an unbound action has no label");
    }

    /// The Mac spellings use symbols; the bundled font must have every one,
    /// or menus show missing-glyph boxes.
    #[test]
    fn the_bundled_font_has_every_mac_symbol() {
        use libgui::{FontRasterizer, FontdueRasterizer};
        let font = FontdueRasterizer::from_bytes(FONT).unwrap();
        let mut glyphs = Vec::new();
        let mut text = format(Shortcut::plain(Key::A).ctrl().alt().shift().logo(), Platform::Mac);
        for (sc, _) in ui_bindings(Platform::Mac).iter() {
            text += &format(sc, Platform::Mac);
        }
        for key in [Key::PageUp, Key::PageDown, Key::Escape, Key::Enter, Key::NumpadEnter] {
            text += key_name(key, Platform::Mac);
        }
        for ch in text.chars() {
            glyphs.clear();
            font.shape(&ch.to_string(), 16.0, &mut glyphs);
            assert_ne!(glyphs[0].glyph, 0, "Inter has no glyph for {ch:?} (U+{:04X})", ch as u32);
        }
    }

    fn key(ui: &mut Ui, key: Key, pressed: bool) {
        ui.push(InputEvent::Key { key, pressed, repeat: false });
    }

    /// App actions go through `consume_shortcut`: one press, one trigger, and
    /// the *other* platform's chord does nothing.
    #[test]
    fn app_actions_trigger_once_and_only_on_their_chord() {
        let mut k = Keymap::new(Platform::Mac);
        k.bind(Chord::primary(Key::S), Act::Save).bind(Chord::primary(Key::Z).shift(), Act::Redo).bind(Chord::primary(Key::Y), Act::Redo);
        let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
        k.install(&mut ui);

        key(&mut ui, Key::SuperLeft, true);
        key(&mut ui, Key::S, true);
        ui.begin_frame(FrameInfo::default());
        assert!(k.triggered(&mut ui, Act::Save));
        assert!(!k.triggered(&mut ui, Act::Save), "one press fired twice");
        let _ = ui.end_frame();
        key(&mut ui, Key::S, false);
        key(&mut ui, Key::SuperLeft, false);

        key(&mut ui, Key::ControlLeft, true);
        key(&mut ui, Key::S, true);
        ui.begin_frame(FrameInfo::default());
        assert!(!k.triggered(&mut ui, Act::Save), "Ctrl+S saved on a Mac");
        let _ = ui.end_frame();
        key(&mut ui, Key::S, false);
        key(&mut ui, Key::ControlLeft, false);

        // Either of an action's chords triggers it; the first is the label.
        key(&mut ui, Key::SuperLeft, true);
        key(&mut ui, Key::Y, true);
        ui.begin_frame(FrameInfo::default());
        assert!(k.triggered(&mut ui, Act::Redo));
        let _ = ui.end_frame();
        assert_eq!(k.label(Act::Redo), "⇧⌘Z");

        // Rebinding replaces every chord.
        k.rebind(Act::Redo, Chord::primary(Key::R));
        assert_eq!(k.shortcuts(Act::Redo).count(), 1);
        assert_eq!(k.action_for(Shortcut::plain(Key::R).logo()), Some(Act::Redo));
    }

    /// The same keys edit a text field differently per platform: Option
    /// deletes a word on a Mac, Ctrl does on Windows, and neither does on the
    /// other.
    #[test]
    fn a_text_field_edits_with_each_platforms_keys() {
        let run = |platform: Platform, mods: &[Key]| -> String {
            let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
            Keymap::<()>::new(platform).install(&mut ui);
            let mut text = String::from("move the cube");
            let frame = |ui: &mut Ui, text: &mut String| {
                ui.begin_frame(FrameInfo::default());
                ui.text_input("f", text, "");
                let _ = ui.end_frame();
            };
            frame(&mut ui, &mut text);
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(200.0, 15.0) });
            ui.push(InputEvent::PointerButton { button: libgui::PointerButton::Primary, pressed: true });
            ui.push(InputEvent::PointerButton { button: libgui::PointerButton::Primary, pressed: false });
            frame(&mut ui, &mut text);
            // End, then modifier+Backspace.
            key(&mut ui, Key::End, true);
            key(&mut ui, Key::End, false);
            for &m in mods {
                key(&mut ui, m, true);
            }
            key(&mut ui, Key::Backspace, true);
            frame(&mut ui, &mut text);
            text
        };
        assert_eq!(run(Platform::Mac, &[Key::AltLeft]), "move the ");
        assert_eq!(run(Platform::Windows, &[Key::ControlLeft]), "move the ");
        assert_eq!(run(Platform::Linux, &[Key::ControlLeft]), "move the ");
        // On a Mac, Cmd+Backspace deletes to the line start instead.
        assert_eq!(run(Platform::Mac, &[Key::SuperLeft]), "");
        // Ctrl+Backspace on a Mac is not word delete: it matches nothing.
        assert_eq!(run(Platform::Mac, &[Key::ControlLeft]), "move the cube");
        // Alt+Backspace on Windows likewise.
        assert_eq!(run(Platform::Windows, &[Key::AltLeft]), "move the cube");
    }
}

/// Where the keyboard is allowed to go on `platform`.
///
/// libgui itself has no opinion — see [`libgui::focus`] — because this is
/// convention rather than fact, and the conventions disagree:
///
/// - **macOS** visits text fields and lists, and nothing else, until Full
///   Keyboard Access is switched on. Clicking a button does not move focus.
/// - **Windows** and **Linux** visit every control, and a click focuses what
///   it pressed.
///
/// A macOS app that ships its own preference, or a kiosk that wants Tab to do
/// nothing at all, builds a [`libgui::FocusPolicy`] by hand instead. Nothing
/// downstream of this function knows what platform it is on.
pub fn focus_policy(platform: Platform) -> libgui::FocusPolicy {
    match platform {
        Platform::Mac => libgui::FocusPolicy {
            text: true,
            controls: false,
            collections: true,
            click_focuses: false,
        },
        Platform::Windows | Platform::Linux => libgui::FocusPolicy {
            text: true,
            controls: true,
            collections: true,
            click_focuses: true,
        },
    }
}

/// [`focus_policy`] with every control reachable, whatever the platform —
/// macOS with Full Keyboard Access on, or an app that has decided a UI nobody
/// can drive from the keyboard is not one it wants to ship.
pub fn full_keyboard_access(platform: Platform) -> libgui::FocusPolicy {
    libgui::FocusPolicy { controls: true, click_focuses: platform != Platform::Mac, ..focus_policy(platform) }
}

#[cfg(test)]
mod focus_tests {
    use super::*;

    #[test]
    fn each_platform_gets_its_own_convention_and_none_is_compiled_in() {
        let mac = focus_policy(Platform::Mac);
        let win = focus_policy(Platform::Windows);
        assert!(!mac.controls, "macOS visits buttons without Full Keyboard Access");
        assert!(!mac.click_focuses, "clicking a button focuses it on macOS");
        assert!(win.controls && win.click_focuses);
        // Every platform is askable from any platform: the argument decides,
        // not the target the binary was built for.
        assert_ne!(mac, win);
        assert_eq!(focus_policy(Platform::Linux), win);
    }

    #[test]
    fn full_keyboard_access_reaches_everything_everywhere() {
        for p in [Platform::Mac, Platform::Windows, Platform::Linux] {
            let f = full_keyboard_access(p);
            assert!(f.text && f.controls && f.collections, "{p:?} left something unreachable");
        }
    }

    #[test]
    fn installing_a_keymap_installs_its_focus_policy_too() {
        let font = include_bytes!("../../../assets/Inter.ttf");
        let mut ui = Ui::new(libgui::Theme::dark(), font).expect("font");
        Keymap::<u8>::new(Platform::Mac).install(&mut ui);
        assert_eq!(ui.focus_policy, focus_policy(Platform::Mac));
        Keymap::<u8>::new(Platform::Windows).install(&mut ui);
        assert_eq!(ui.focus_policy, focus_policy(Platform::Windows));
    }
}
