//! Keyboard navigation **inside** a collection.
//!
//! Tab has always moved focus between widgets. Moving within one — down a
//! hierarchy, through a list, across a tree — did not exist, which is the
//! first thing anyone reaches for in a professional tool and the half of
//! accessibility that AccessKit does not supply: exposing a tree to a screen
//! reader does not make it navigable.
//!
//! `FocusKind::Collection` has promised this since it was written ("one stop
//! from outside; the arrows move within it once it has focus"). These tests
//! are that promise.

use libgui::*;

/// The bindings this test presses. Written out here rather than taken from
/// `libgui_keymap`, because the core must work with *any* table: a test that
/// borrowed the keymap's choices would be testing the keymap.
///
/// The arrows are deliberately bound twice — a caret motion and a navigation
/// — which is exactly the arrangement a real table uses.
fn bindings() -> KeyBindings {
    let mut b = KeyBindings::new();
    for (key, nav, motion) in [
        (Key::ArrowDown, Nav::Next, Motion::Down),
        (Key::ArrowUp, Nav::Previous, Motion::Up),
        (Key::ArrowRight, Nav::Expand, Motion::Right),
        (Key::ArrowLeft, Nav::Collapse, Motion::Left),
    ] {
        b.bind(Shortcut::plain(key), UiAction::Move { motion, select: false });
        b.bind(Shortcut::plain(key), UiAction::Navigate(nav));
    }
    b.bind(Shortcut::plain(Key::Home), UiAction::Navigate(Nav::First));
    b.bind(Shortcut::plain(Key::End), UiAction::Navigate(Nav::Last));
    b.bind(Shortcut::plain(Key::PageDown), UiAction::Navigate(Nav::PageNext));
    b.bind(Shortcut::plain(Key::PageUp), UiAction::Navigate(Nav::PagePrevious));
    b.bind(Shortcut::plain(Key::Tab), UiAction::FocusNext);
    b.bind(Shortcut::plain(Key::Tab).shift(), UiAction::FocusPrevious);
    b.bind(Shortcut::plain(Key::Enter), UiAction::Submit);
    b
}

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const INFO: FrameInfo = FrameInfo { screen_size: Vec2::new(400.0, 600.0), scale: 1.0, dt: 1.0 / 60.0 };
const ROWS: usize = 8;

struct World {
    ui: Ui,
    /// What the app keeps: the selected row, driven by the cursor.
    selected: usize,
    nav: Option<NavResponse>,
    /// A leading button, so there is somewhere else for focus to be.
    before: bool,
}

impl World {
    fn new() -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        ui.set_key_bindings(bindings());
        let mut w = Self { ui, selected: 0, nav: None, before: false };
        w.frame();
        w.frame();
        w
    }

    fn frame(&mut self) {
        self.ui.begin_frame(INFO);
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        let mut nav = None;
        let mut selected = self.selected;
        let mut before = false;
        self.ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            before = ui.button("Before").focused;
            let n = ui.open_collection("rows", ROWS);
            if n.moved {
                selected = n.cursor;
            }
            for i in 0..ROWS {
                if ui.selectable_keyed(i, &format!("Row {i}"), selected == i).clicked {
                    selected = i;
                    ui.set_cursor(n.id, i);
                }
            }
            ui.close_collection();
            nav = Some(n);
        });
        (self.selected, self.nav, self.before) = (selected, nav, before);
        let _ = self.ui.end_frame();
    }

    fn nav(&self) -> NavResponse {
        self.nav.expect("no collection response")
    }

    fn key(&mut self, key: Key) {
        self.ui.push(InputEvent::Key { key, pressed: true, repeat: false });
        self.frame();
        self.ui.push(InputEvent::Key { key, pressed: false, repeat: false });
        self.frame();
    }

    fn tab(&mut self) {
        self.key(Key::Tab);
    }

    /// Put focus on the collection: one Tab past the leading button.
    fn focus_rows(&mut self) {
        while !self.nav().focused {
            self.tab();
            assert!(self.ui.frame_cost().nodes > 0);
        }
    }
}

/// **The point.** Once the list has focus, Down moves the cursor through it.
#[test]
fn the_arrows_move_the_cursor_inside_a_focused_list() {
    let mut w = World::new();
    w.focus_rows();
    assert_eq!(w.nav().cursor, 0);

    w.key(Key::ArrowDown);
    assert_eq!(w.nav().cursor, 1);
    assert_eq!(w.selected, 1, "the app's selection did not follow the cursor");

    w.key(Key::ArrowDown);
    w.key(Key::ArrowDown);
    assert_eq!(w.nav().cursor, 3);

    w.key(Key::ArrowUp);
    assert_eq!(w.nav().cursor, 2);
}

/// **The other half, and the reason rows stop being focus stops.** Tab leaves
/// the collection entirely rather than stepping into its next row. A
/// thousand-row list is one stop, not a thousand.
#[test]
fn tab_steps_over_a_collection_not_through_it() {
    let mut w = World::new();
    // Tab onto the leading button, then count how many stops it takes to come
    // back to it. That is the whole ring: a button and a list, so two. With
    // the rows as their own stops it was nine, and a real hierarchy panel of a
    // thousand nodes would be a thousand and one.
    w.tab();
    assert!(w.before, "the button never took focus");
    let mut cycle = 0;
    loop {
        w.tab();
        cycle += 1;
        assert!(cycle <= 20, "the focus ring never came back round: {cycle} stops and counting");
        if w.before {
            break;
        }
    }
    assert_eq!(cycle, 2, "the ring has {cycle} stops: the list's rows are still individual stops");
}

/// The cursor does not run off either end. A list that wrapped by default
/// would send someone hunting for where the selection went.
#[test]
fn the_cursor_stops_at_the_ends() {
    let mut w = World::new();
    w.focus_rows();
    for _ in 0..ROWS + 4 {
        w.key(Key::ArrowDown);
    }
    assert_eq!(w.nav().cursor, ROWS - 1);
    for _ in 0..ROWS + 4 {
        w.key(Key::ArrowUp);
    }
    assert_eq!(w.nav().cursor, 0);
}

/// Home and End, and Page Down by the collection's own page size — which the
/// collection knows and the keymap cannot.
#[test]
fn home_end_and_page_reach_the_ends() {
    let mut w = World::new();
    w.focus_rows();
    w.key(Key::End);
    assert_eq!(w.nav().cursor, ROWS - 1);
    w.key(Key::Home);
    assert_eq!(w.nav().cursor, 0);
    // The default page is larger than this list, so one Page Down lands on the
    // last row rather than past it.
    w.key(Key::PageDown);
    assert_eq!(w.nav().cursor, ROWS - 1);
    w.key(Key::PageUp);
    assert_eq!(w.nav().cursor, 0);
}

/// A tree's Right and Left are reported, not acted on: libgui does not know
/// your tree's shape, and a widget that guessed would be wrong about it.
#[test]
fn expand_and_collapse_are_reported_for_the_app() {
    let mut w = World::new();
    w.focus_rows();
    w.ui.push(InputEvent::Key { key: Key::ArrowRight, pressed: true, repeat: false });
    w.frame();
    assert!(w.nav().expand, "Right did not reach the collection");
    assert!(!w.nav().collapse);

    w.ui.push(InputEvent::Key { key: Key::ArrowLeft, pressed: true, repeat: false });
    w.frame();
    assert!(w.nav().collapse, "Left did not reach the collection");
}

/// Enter activates the row the cursor is on, the way a double click opens it.
#[test]
fn enter_activates_the_cursor_row() {
    let mut w = World::new();
    w.focus_rows();
    w.key(Key::ArrowDown);
    w.ui.push(InputEvent::Key { key: Key::Enter, pressed: true, repeat: false });
    w.frame();
    assert!(w.nav().activated);
    assert_eq!(w.nav().cursor, 1, "activating moved the cursor");
}

/// Clicking a row leaves the keyboard where the pointer left off, so arrowing
/// after a click continues from the clicked row rather than jumping back.
#[test]
fn a_click_moves_the_cursor_too() {
    let mut w = World::new();
    w.focus_rows();
    // Row 4's rect, from the frame that laid it out.
    let row = w.ui.rect_of(Id::new("root").with(("selectable", 4u64))).unwrap_or_default();
    let at = if row.w > 0.0 { row.center() } else { Vec2::new(200.0, 40.0 + 4.0 * 24.0) };
    w.ui.push(InputEvent::PointerMoved { pos: at });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    w.frame();
    let clicked = w.selected;
    assert!(clicked > 0, "the click did not land on a row");
    // One more frame: the response was built before the click handler moved
    // the cursor, so the new value is next frame's.
    w.frame();
    assert_eq!(w.nav().cursor, clicked, "the cursor stayed where the keyboard had left it");
}

/// Nothing moves while the collection does not have focus: the arrows belong
/// to whatever *is* focused, which is usually a text field's caret.
#[test]
fn an_unfocused_collection_ignores_the_arrows() {
    let mut w = World::new();
    w.tab(); // the button, not the list
    assert!(!w.nav().focused);
    w.key(Key::ArrowDown);
    w.key(Key::ArrowDown);
    assert_eq!(w.nav().cursor, 0, "an unfocused list ate the arrows");
}

/// The same arrow key still moves a caret. Binding one chord to both is the
/// deliberate design; a regression here would be silent, because the list
/// would keep working while every text field stopped.
#[test]
fn the_arrows_still_move_a_caret_in_a_text_field() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.set_key_bindings(bindings());
    let mut text = String::from("hello");
    let frame = |ui: &mut Ui, text: &mut String| {
        ui.begin_frame(INFO);
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        let mut c = (0, 0);
        ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            let r = ui.text_area("field", text, 3);
            if r.focused {
                c = r.caret;
            }
        });
        let _ = ui.end_frame();
        c
    };
    frame(&mut ui, &mut text);
    // Focus the field and put the caret home.
    ui.push(InputEvent::Key { key: Key::Tab, pressed: true, repeat: false });
    frame(&mut ui, &mut text);
    ui.push(InputEvent::Key { key: Key::Tab, pressed: false, repeat: false });
    frame(&mut ui, &mut text);
    ui.push(InputEvent::Key { key: Key::ArrowRight, pressed: true, repeat: false });
    let caret = frame(&mut ui, &mut text);
    assert_eq!(caret.1, 1, "Right stopped moving the caret once it also drove a list");
}

/// A focused collection is *visible* as focused. It is a focus stop without
/// being a node, so the ring — which is drawn around the focused node — had
/// nowhere to go, and tabbing onto a list lit nothing up. It borrows the ring
/// of the container it was opened in.
///
/// "A UI that cannot be operated from the keyboard is broken" is this library's
/// own claim; a focus state nobody can see is half of that breakage.
#[test]
fn a_focused_collection_shows_a_focus_ring() {
    /// Quads drawn in the theme's focus-ring colour.
    fn rings(ui: &mut Ui) -> usize {
        let want = ui.theme.palette.focus_ring.to_array();
        ui.begin_frame(INFO);
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            ui.button("Before");
            let list = Layout::column().width(Size::Grow(1.0)).height(Size::Fit);
            ui.container_id(Id::new("list"), list, Frame::none(), |ui| {
                let n = ui.open_collection("rows", ROWS);
                let _ = n;
                for i in 0..ROWS {
                    ui.selectable_keyed(i, &format!("Row {i}"), false);
                }
                ui.close_collection();
            });
        });
        let out = ui.end_frame();
        out.draw.instances.iter().filter(|i| i.border_color == want).count()
    }

    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.set_key_bindings(bindings());
    rings(&mut ui);
    rings(&mut ui);
    let idle = rings(&mut ui);

    // Tab twice: past the button, onto the list.
    for _ in 0..2 {
        ui.push(InputEvent::Key { key: Key::Tab, pressed: true, repeat: false });
        rings(&mut ui);
        ui.push(InputEvent::Key { key: Key::Tab, pressed: false, repeat: false });
        rings(&mut ui);
    }
    assert!(rings(&mut ui) > idle, "tabbing onto the list drew no focus ring");
}
