//! Multi-select: the gesture, the anchor, and who owns the set.
//!
//! A CAD model browser picking two bodies for a Join is the case this exists
//! for. libgui never learns what a row *is* — the set stays the app's — but the
//! anchor a Shift-click extends from is bookkeeping every application would
//! otherwise write, and get subtly wrong.
//!
//! In `libgui_keymap` rather than `libgui`, because half the answer is a
//! platform convention: Command toggles on macOS, Control everywhere else.

use libgui::*;
use libgui_keymap::{select_kind, Platform};
use std::collections::BTreeSet;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const ROWS: usize = 8;

fn mods(shift: bool, ctrl: bool, logo: bool) -> Modifiers {
    Modifiers { shift, ctrl, logo, alt: false }
}

/// A model browser: rows, a selected set, and clicks applied through
/// `Ui::select` exactly as an app would.
struct Browser {
    ui: Ui,
    picked: BTreeSet<usize>,
}

impl Browser {
    fn new() -> Self {
        let mut w = Self { ui: Ui::new(Theme::dark(), FONT).expect("font"), picked: BTreeSet::new() };
        w.frame(None, Modifiers::default());
        w.frame(None, Modifiers::default());
        w
    }

    /// One frame. `click` is the row the user hit, if any.
    fn frame(&mut self, click: Option<usize>, m: Modifiers) {
        self.ui.begin_frame(FrameInfo::default());
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        let picked = &mut self.picked;
        self.ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            let nav = ui.open_collection("model", ROWS);
            for i in 0..ROWS {
                ui.selectable_keyed(i, &format!("Body {i}"), picked.contains(&i));
                if click == Some(i) {
                    match ui.select(nav.id, i, select_kind(Platform::Windows, &m)) {
                        Selection::Only(i) => {
                            picked.clear();
                            picked.insert(i);
                        }
                        Selection::Toggle(i) => {
                            if !picked.remove(&i) {
                                picked.insert(i);
                            }
                        }
                        Selection::Range(r) => {
                            picked.clear();
                            picked.extend(r);
                        }
                    }
                }
            }
            ui.close_collection();
        });
        let _ = self.ui.end_frame();
    }

    fn click(&mut self, i: usize, m: Modifiers) {
        self.frame(Some(i), m);
    }

    fn picked(&self) -> Vec<usize> {
        self.picked.iter().copied().collect()
    }
}

/// **The case this exists for:** pick two bodies for a Join.
#[test]
fn ctrl_click_picks_a_second_body() {
    let mut b = Browser::new();
    b.click(2, mods(false, false, false));
    assert_eq!(b.picked(), vec![2]);
    b.click(5, mods(false, true, false));
    assert_eq!(b.picked(), vec![2, 5], "Ctrl-click did not add a second body");
}

/// A plain click replaces, however much was selected.
#[test]
fn a_plain_click_replaces_the_selection() {
    let mut b = Browser::new();
    b.click(1, mods(false, true, false));
    b.click(3, mods(false, true, false));
    b.click(6, mods(false, true, false));
    assert_eq!(b.picked(), vec![1, 3, 6]);
    b.click(4, mods(false, false, false));
    assert_eq!(b.picked(), vec![4], "a plain click did not clear the rest");
}

/// Ctrl-click removes as well as adds.
#[test]
fn ctrl_click_deselects_what_is_selected() {
    let mut b = Browser::new();
    b.click(2, mods(false, false, false));
    b.click(3, mods(false, true, false));
    assert_eq!(b.picked(), vec![2, 3]);
    b.click(2, mods(false, true, false));
    assert_eq!(b.picked(), vec![3], "Ctrl-click did not remove an already-selected row");
}

/// Shift takes everything from the anchor, in either direction.
#[test]
fn shift_click_takes_a_range_either_way() {
    let mut b = Browser::new();
    b.click(2, mods(false, false, false));
    b.click(5, mods(true, false, false));
    assert_eq!(b.picked(), vec![2, 3, 4, 5]);

    b.click(5, mods(false, false, false));
    b.click(1, mods(true, false, false));
    assert_eq!(b.picked(), vec![1, 2, 3, 4, 5], "a backwards range came out wrong");
}

/// **The fiddly part.** Dragging a Shift-click up and down grows and shrinks
/// *one* range from a fixed anchor, rather than ratcheting a new one from
/// wherever the last Shift-click landed.
#[test]
fn the_anchor_does_not_move_while_a_range_is_dragged_out() {
    let mut b = Browser::new();
    b.click(3, mods(false, false, false));
    b.click(6, mods(true, false, false));
    assert_eq!(b.picked(), vec![3, 4, 5, 6]);
    b.click(4, mods(true, false, false));
    assert_eq!(b.picked(), vec![3, 4], "the anchor moved: the range ratcheted instead of shrinking");
    b.click(1, mods(true, false, false));
    assert_eq!(b.picked(), vec![1, 2, 3], "the range did not flip around a fixed anchor");
}

/// A toggle *does* move the anchor, so the next Shift-click extends from what
/// was last touched — what every file manager does.
#[test]
fn a_toggle_moves_the_anchor() {
    let mut b = Browser::new();
    b.click(1, mods(false, false, false));
    b.click(5, mods(false, true, false));
    assert_eq!(b.picked(), vec![1, 5]);
    b.click(7, mods(true, false, false));
    assert_eq!(b.picked(), vec![5, 6, 7], "Shift extended from the click before the toggle");
}

/// Shift wins when both are held, which is what Finder and Explorer do.
#[test]
fn shift_beats_the_toggle_modifier() {
    assert_eq!(select_kind(Platform::Windows, &mods(true, true, false)), SelectKind::Range);
    assert_eq!(select_kind(Platform::Mac, &mods(true, false, true)), SelectKind::Range);
}

/// The toggle modifier is the platform's: Command on macOS, Control elsewhere.
/// Getting this backwards makes a Mac app feel wrong in a way users notice and
/// cannot name.
#[test]
fn the_toggle_modifier_follows_the_platform() {
    assert_eq!(select_kind(Platform::Mac, &mods(false, false, true)), SelectKind::Toggle);
    assert_eq!(select_kind(Platform::Mac, &mods(false, true, false)), SelectKind::Replace);
    for p in [Platform::Windows, Platform::Linux] {
        assert_eq!(select_kind(p, &mods(false, true, false)), SelectKind::Toggle);
        assert_eq!(select_kind(p, &mods(false, false, true)), SelectKind::Replace);
    }
}

/// A `Response` carries the modifiers of the click, so the row that was hit
/// can resolve its own gesture without reaching elsewhere for the state.
#[test]
fn a_response_reports_the_modifiers_of_its_click() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut seen = Modifiers::default();
    let mut rect = Rect::default();
    for i in 0..5 {
        ui.begin_frame(FrameInfo::default());
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            let r = ui.selectable_keyed(0, "Body", false);
            rect = r.rect;
            if r.clicked {
                seen = r.modifiers;
            }
        });
        let _ = ui.end_frame();
        match i {
            0 => ui.push(InputEvent::PointerMoved { pos: rect.center() }),
            1 => {
                ui.push(InputEvent::ModifiersChanged(mods(true, true, false)));
                ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
            }
            2 => ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false }),
            _ => {}
        }
    }
    assert!(seen.shift && seen.ctrl, "the click's modifiers did not reach the response: {seen:?}");
}
