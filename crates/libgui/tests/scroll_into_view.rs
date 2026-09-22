//! Keyboard focus that cannot be seen is not focus.
//!
//! Tabbing into a form taller than its viewport used to leave the focus ring
//! drawn somewhere off screen: the scroll area never followed, so any form or
//! list longer than the window was unusable from the keyboard. Measured before
//! the fix, twelve Tabs left the focused button at y=418 in a 200 px viewport.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const INFO: FrameInfo = FrameInfo { screen_size: Vec2::new(300.0, 200.0), scale: 1.0, dt: 1.0 / 60.0 };
const ROWS: usize = 30;

struct World {
    ui: Ui,
    /// Which button has focus, and where it was drawn.
    focused: Option<(usize, Rect)>,
    /// The scroll area's own rect, so "visible" is measured against the
    /// viewport rather than the window.
    view: Rect,
}

impl World {
    fn new() -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let mut b = KeyBindings::new();
        b.bind(Shortcut::plain(Key::Tab), UiAction::FocusNext);
        b.bind(Shortcut::plain(Key::Tab).shift(), UiAction::FocusPrevious);
        ui.set_key_bindings(b);
        let mut w = Self { ui, focused: None, view: Rect::new(0.0, 0.0, 0.0, 0.0) };
        w.frame();
        w.frame();
        w
    }

    fn frame(&mut self) {
        self.ui.begin_frame(INFO);
        let mut focused = None;
        self.ui.scroll_area("sc", |ui| {
            for i in 0..ROWS {
                let r = ui.button(&format!("Button {i}"));
                if r.focused {
                    focused = Some((i, r.rect));
                }
            }
        });
        self.view = self.ui.rect_of(Id::new("root").with(("scroll", "sc"))).unwrap_or_default();
        self.focused = focused;
        let _ = self.ui.end_frame();
    }

    fn tab(&mut self) {
        self.ui.push(InputEvent::Key { key: Key::Tab, pressed: true, repeat: false });
        self.frame();
        self.ui.push(InputEvent::Key { key: Key::Tab, pressed: false, repeat: false });
        self.frame();
    }

    fn shift_tab(&mut self) {
        // The modifier has to arrive before the key, or the chord resolves
        // without it.
        self.ui.push(InputEvent::ModifiersChanged(Modifiers { shift: true, ..Modifiers::NONE }));
        self.ui.push(InputEvent::Key { key: Key::Tab, pressed: true, repeat: false });
        self.frame();
        self.ui.push(InputEvent::Key { key: Key::Tab, pressed: false, repeat: false });
        self.ui.push(InputEvent::ModifiersChanged(Modifiers::NONE));
        self.frame();
    }

    /// Let an eased scroll arrive.
    fn settle(&mut self) {
        for _ in 0..40 {
            self.frame();
        }
    }

    /// Is the focused widget inside the viewport it lives in?
    fn focus_visible(&self) -> bool {
        match self.focused {
            Some((_, r)) => r.y >= self.view.y - 0.5 && r.y + r.h <= self.view.y + self.view.h + 0.5,
            None => false,
        }
    }
}

/// **The fix.** Tab far enough down a scrolling form and the focused control
/// is still on screen, every step of the way.
#[test]
fn tabbing_below_the_fold_scrolls_the_focus_into_view() {
    let mut w = World::new();
    // The area fills the window; what makes it scroll is thirty rows inside a
    // 200 px viewport.
    assert!(w.view.h > 0.0 && w.view.h <= 200.0, "no viewport: {:?}", w.view);

    for step in 1..=14 {
        w.tab();
        w.settle();
        let (i, r) = w.focused.expect("nothing has focus");
        assert_eq!(i, (step - 1) % ROWS, "focus did not advance in order");
        assert!(w.focus_visible(), "after {step} tabs, button {i} is at y={} and the viewport is {:?}", r.y, w.view);
    }
}

/// And back up again: Shift+Tab from the top wraps to the last control, which
/// is the furthest thing from the viewport there is.
#[test]
fn shift_tab_to_the_far_end_scrolls_there_too() {
    let mut w = World::new();
    w.shift_tab();
    w.settle();
    let (i, _) = w.focused.expect("nothing has focus");
    assert_eq!(i, ROWS - 1, "Shift+Tab did not wrap to the last control");
    assert!(w.focus_visible(), "the last control is off screen: {:?} in {:?}", w.focused.map(|f| f.1), w.view);

    // And forward again, wrapping to the first, scrolls back to the top.
    w.tab();
    w.settle();
    assert_eq!(w.focused.map(|f| f.0), Some(0));
    assert!(w.focus_visible(), "wrapping to the first control did not scroll back up");
}

/// A widget already on screen is left alone. Scrolling to something visible
/// would yank the view out from under the reader for no reason — the whole
/// point of "into view" is that it is a no-op when it already is.
///
/// Measured against a *third* row that is not the one taking focus: if the
/// view moved at all, every row moved with it.
#[test]
fn a_visible_widget_is_not_scrolled_to() {
    let mut w = World::new();
    /// Where row 3 is drawn — a landmark that only moves if the view does.
    fn landmark(w: &mut World) -> f32 {
        w.ui.rect_of(Id::new("root").with(("scroll", "sc")).with(("button", "Button 3"))).unwrap_or_default().y
    }

    w.tab(); // button 0, at the top and visible
    w.settle();
    assert!(w.focus_visible());
    let before = landmark(&mut w);
    assert!(before != 0.0, "the landmark row was never laid out");

    w.tab(); // button 1, also already visible
    w.settle();
    assert_eq!(w.focused.map(|f| f.0), Some(1), "focus did not advance");
    assert_eq!(landmark(&mut w), before, "focusing an already-visible widget scrolled the view");
}

/// `scroll_to` is available to the app for cursors libgui does not own — a
/// collection's current row, a search hit, a selection made in code.
#[test]
fn an_app_can_scroll_to_a_widget_itself() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut seen = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut view = Rect::new(0.0, 0.0, 0.0, 0.0);

    let frame = |ui: &mut Ui, want: Option<usize>| {
        ui.begin_frame(INFO);
        let mut r_at = Rect::new(0.0, 0.0, 0.0, 0.0);
        ui.scroll_area("sc", |ui| {
            for i in 0..ROWS {
                let r = ui.selectable_keyed(i, &format!("Row {i}"), false);
                if want == Some(i) {
                    ui.scroll_to(r.id);
                    r_at = r.rect;
                }
            }
        });
        let v = ui.rect_of(Id::new("root").with(("scroll", "sc"))).unwrap_or_default();
        let _ = ui.end_frame();
        (r_at, v)
    };
    frame(&mut ui, None);
    frame(&mut ui, None);

    // Ask for a row well past the bottom, then let the scroll arrive.
    for _ in 0..50 {
        let (r, v) = frame(&mut ui, Some(25));
        (seen, view) = (r, v);
    }
    assert!(view.h > 0.0);
    assert!(
        seen.y >= view.y - 0.5 && seen.y + seen.h <= view.y + view.h + 0.5,
        "row 25 is at y={} and the viewport is {:?}",
        seen.y,
        view
    );
}
