//! Keyboard focus: the mechanism, and none of the policy.
//!
//! Which controls the keyboard visits is a platform convention, and platforms
//! disagree flatly. macOS visits text fields and lists, and nothing else,
//! until Full Keyboard Access is switched on; Windows and Linux visit every
//! control; a kiosk or a hardware panel may want the keyboard to visit exactly
//! one thing. Clicking a button focuses it on Windows and does not on macOS.
//!
//! So libgui has no opinion. It knows *what each widget is* — text, a control,
//! a collection — and it asks a [`FocusPolicy`] the app sets whether that kind
//! is a stop. [`libgui_keymap`](https://docs.rs/libgui_keymap) carries the
//! per-platform defaults, exactly as it carries the key bindings, and a host
//! with its own conventions writes its own.
//!
//! The keys are not libgui's either. A focused widget is activated by
//! [`UiAction::Submit`](crate::UiAction::Submit) and left by
//! [`UiAction::Cancel`](crate::UiAction::Cancel); focus moves on
//! [`FocusNext`](crate::UiAction::FocusNext) and
//! [`FocusPrevious`](crate::UiAction::FocusPrevious). What chord produces any
//! of those is the keymap's, and a host with no keyboard at all can send them
//! directly with [`InputEvent::Action`](crate::InputEvent::Action) — a gamepad,
//! a foot pedal, an accessibility switch.

/// What sort of thing a widget is, as far as the keyboard is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FocusKind {
    /// Text entry. Every platform's keyboard visits these.
    Text,
    /// Something you press or adjust: a button, toggle, checkbox, radio,
    /// slider, combo, segmented control.
    Control,
    /// A list, tree, table or menu. One stop from outside; the arrows move
    /// within it once it has focus.
    Collection,
}

/// Which kinds of widget the keyboard visits, and what a click does to focus.
///
/// Neutral by default — everything is reachable — because a UI that cannot be
/// operated from the keyboard is broken, and a library that silently decides
/// otherwise on your behalf is worse than one that asks. Narrow it to match a
/// platform with [`libgui_keymap`](https://docs.rs/libgui_keymap), or by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FocusPolicy {
    pub text: bool,
    pub controls: bool,
    pub collections: bool,
    /// Pressing a widget gives it keyboard focus. True on Windows and Linux,
    /// false on macOS, where clicking a button leaves focus where it was.
    /// Text fields take focus on click whatever this says: there is nowhere
    /// else for the caret to go.
    pub click_focuses: bool,
}

impl Default for FocusPolicy {
    fn default() -> Self {
        Self { text: true, controls: true, collections: true, click_focuses: true }
    }
}

impl FocusPolicy {
    /// Nothing but text fields, the way a keyboard behaves on macOS with Full
    /// Keyboard Access off. Still a policy, still the app's to choose.
    pub fn text_only() -> Self {
        Self { text: true, controls: false, collections: false, click_focuses: false }
    }

    pub fn accepts(&self, kind: FocusKind) -> bool {
        match kind {
            FocusKind::Text => self.text,
            FocusKind::Control => self.controls,
            FocusKind::Collection => self.collections,
        }
    }
}

/// What the keyboard did to a widget that has focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyResponse {
    /// It has keyboard focus.
    pub focused: bool,
    /// It was activated from the keyboard this frame.
    pub activated: bool,
}
