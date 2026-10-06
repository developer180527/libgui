//! The keyboard, through the bindings an app actually installs.
//!
//! Each test drives real key events through `ui_bindings`, so what is
//! checked is what a user gets — not a binding chosen to make the test pass.

use libgui::*;
use libgui_keymap::{full_keyboard_access, ui_bindings, Platform};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const ALL: [Platform; 3] = [Platform::Mac, Platform::Windows, Platform::Linux];

fn ui(platform: Platform) -> Ui {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.set_key_bindings(ui_bindings(platform));
    ui.focus_policy = full_keyboard_access(platform);
    ui
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(400.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 }
}

/// Press and release `key` with `mods` held, running `frame` after each event.
fn press(ui: &mut Ui, key: Key, shift: bool, mut frame: impl FnMut(&mut Ui)) {
    if shift {
        ui.push(InputEvent::ModifiersChanged(Modifiers { shift: true, ..Default::default() }));
    }
    ui.push(InputEvent::Key { key, pressed: true, repeat: false });
    if key == Key::Space {
        ui.push(InputEvent::Text(" ".into()));
    }
    frame(ui);
    ui.push(InputEvent::Key { key, pressed: false, repeat: false });
    if shift {
        ui.push(InputEvent::ModifiersChanged(Modifiers::default()));
    }
    frame(ui);
}

#[test]
fn space_presses_the_focused_button_everywhere_and_enter_where_that_is_the_convention() {
    for platform in ALL {
        for (key, expect) in [(Key::Space, true), (Key::Enter, platform != Platform::Mac)] {
            let mut ui = ui(platform);
            let mut clicks = 0;
            let mut frame = |ui: &mut Ui| {
                ui.begin_frame(info());
                clicks += ui.button("OK").clicked as usize;
                let _ = ui.end_frame();
            };
            frame(&mut ui);
            press(&mut ui, Key::Tab, false, &mut frame);
            press(&mut ui, key, false, &mut frame);
            assert_eq!(clicks == 1, expect, "{platform:?}: {key:?} pressed the focused button {clicks} times");
        }
    }
}

#[test]
fn space_in_a_text_field_is_a_space() {
    for platform in ALL {
        let mut ui = ui(platform);
        let mut text = String::from("a");
        let mut submitted = false;
        let mut frame = |ui: &mut Ui| {
            ui.begin_frame(info());
            let r = ui.text_input("name", &mut text, "");
            submitted |= r.submitted;
            let _ = ui.end_frame();
        };
        frame(&mut ui);
        press(&mut ui, Key::Tab, false, &mut frame);
        press(&mut ui, Key::End, false, &mut frame);
        press(&mut ui, Key::Space, false, &mut frame);
        assert!(!submitted, "{platform:?}: a space committed the field");
        assert_eq!(text, "a ", "{platform:?}: the space was not typed");
    }
}

/// A File menu with a disabled row, a separator and a submenu, recording what
/// was chosen.
#[derive(Default)]
struct Menu {
    chosen: Vec<&'static str>,
    open: bool,
}

fn file_menu(ui: &mut Ui, m: &mut Menu) {
    ui.row(|ui| {
        let r = ui.menu_button("File", |ui| {
            if ui.menu_item("New").clicked {
                m.chosen.push("New");
            }
            if ui.menu_item_ex("Recent", None, false).clicked {
                m.chosen.push("Recent");
            }
            ui.menu_separator();
            ui.submenu("Export", |ui| {
                if ui.menu_item("PNG").clicked {
                    m.chosen.push("PNG");
                }
                if ui.menu_item("SVG").clicked {
                    m.chosen.push("SVG");
                }
            });
            if ui.menu_item("Quit").clicked {
                m.chosen.push("Quit");
            }
        });
        m.open = r.is_some();
    });
}

fn menu_world(platform: Platform) -> (Ui, Menu) {
    let mut ui = ui(platform);
    let mut m = Menu::default();
    for _ in 0..2 {
        ui.begin_frame(info());
        file_menu(&mut ui, &mut m);
        let _ = ui.end_frame();
    }
    (ui, m)
}

fn keys(ui: &mut Ui, m: &mut Menu, seq: &[Key]) {
    for &k in seq {
        press(ui, k, false, |ui| {
            ui.begin_frame(info());
            file_menu(ui, m);
            let _ = ui.end_frame();
        });
    }
}

#[test]
fn a_menu_is_opened_walked_and_chosen_from_the_keyboard() {
    for platform in ALL {
        // Tab to File, Space opens it on the first row, Enter chooses it.
        let (mut ui, mut m) = menu_world(platform);
        keys(&mut ui, &mut m, &[Key::Tab, Key::Space]);
        assert!(m.open, "{platform:?}: Space did not open the focused menu");
        keys(&mut ui, &mut m, &[Key::Enter]);
        assert_eq!(m.chosen, ["New"], "{platform:?}: the first row was not highlighted on opening");
        assert!(!m.open, "{platform:?}: choosing did not close the menu");

        // Down skips the disabled row and the separator.
        let (mut ui, mut m) = menu_world(platform);
        keys(&mut ui, &mut m, &[Key::Tab, Key::Space, Key::ArrowDown, Key::ArrowDown, Key::Enter]);
        assert_eq!(m.chosen, ["Quit"], "{platform:?}: Down landed on a disabled row or did not skip Export's");

        // Up from the first row wraps to the last.
        let (mut ui, mut m) = menu_world(platform);
        keys(&mut ui, &mut m, &[Key::Tab, Key::Space, Key::ArrowUp, Key::Enter]);
        assert_eq!(m.chosen, ["Quit"], "{platform:?}: Up did not wrap");
    }
}

#[test]
fn right_opens_a_submenu_and_left_backs_out_of_it() {
    for platform in ALL {
        let (mut ui, mut m) = menu_world(platform);
        // New -> Export, Right opens it on PNG, Down -> SVG, Enter.
        keys(&mut ui, &mut m, &[Key::Tab, Key::Space, Key::ArrowDown, Key::ArrowRight, Key::ArrowDown, Key::Enter]);
        assert_eq!(m.chosen, ["SVG"], "{platform:?}: the submenu was not walked");
        assert!(!m.open, "{platform:?}: choosing in a submenu left the menu open");

        // Left closes the submenu and leaves the parent on Export.
        let (mut ui, mut m) = menu_world(platform);
        keys(&mut ui, &mut m, &[Key::Tab, Key::Space, Key::ArrowDown, Key::ArrowRight, Key::ArrowLeft]);
        assert!(m.open, "{platform:?}: Left closed the whole menu");
        keys(&mut ui, &mut m, &[Key::ArrowDown, Key::Enter]);
        assert_eq!(m.chosen, ["Quit"], "{platform:?}: after Left the highlight was not on Export");
    }
}

#[test]
fn escape_backs_out_one_level_at_a_time() {
    let (mut ui, mut m) = menu_world(Platform::Windows);
    keys(&mut ui, &mut m, &[Key::Tab, Key::Space, Key::ArrowDown, Key::ArrowRight, Key::Escape]);
    assert!(m.open, "Escape in a submenu closed the whole menu");
    keys(&mut ui, &mut m, &[Key::Escape]);
    assert!(!m.open, "a second Escape did not close the menu");
    // Focus stayed on File: Space opens it again.
    keys(&mut ui, &mut m, &[Key::Space]);
    assert!(m.open, "Escape took focus away from the menu button");
}

/// The menu has the keyboard while it is open, whatever was built first: a
/// list with focus, built before its context menu, does not take the arrows.
#[test]
fn an_open_context_menu_has_the_arrows_not_the_list_under_it() {
    let mut ui = ui(Platform::Windows);
    let cursor = std::cell::Cell::new(0usize);
    let row0 = std::cell::Cell::new(Rect::default());
    let mut chosen = Vec::new();
    let frame = |ui: &mut Ui, chosen: &mut Vec<&'static str>| {
        ui.begin_frame(info());
        let nav = ui.open_collection("rows", 5);
        cursor.set(nav.cursor);
        for i in 0..5 {
            let r = ui.selectable_keyed(i, "Row", nav.cursor == i);
            if i == 0 {
                row0.set(r.rect);
                ui.context_menu(&r, |ui| {
                    if ui.menu_item("Rename").clicked {
                        chosen.push("Rename");
                    }
                    if ui.menu_item("Delete").clicked {
                        chosen.push("Delete");
                    }
                });
            }
        }
        ui.close_collection();
        let _ = ui.end_frame();
    };
    frame(&mut ui, &mut chosen);
    press(&mut ui, Key::Tab, false, |ui| frame(ui, &mut chosen));
    press(&mut ui, Key::ArrowDown, false, |ui| frame(ui, &mut chosen));
    assert_eq!(cursor.get(), 1, "the list never had the keyboard");
    // Right-click the first row to open its menu.
    let at = row0.get().center();
    ui.push(InputEvent::PointerMoved { pos: at });
    ui.push(InputEvent::PointerButton { button: PointerButton::Secondary, pressed: true });
    frame(&mut ui, &mut chosen);
    ui.push(InputEvent::PointerButton { button: PointerButton::Secondary, pressed: false });
    frame(&mut ui, &mut chosen);
    for k in [Key::ArrowDown, Key::ArrowDown, Key::Enter] {
        press(&mut ui, k, false, |ui| frame(ui, &mut chosen));
    }
    assert_eq!(cursor.get(), 1, "the list under the menu moved its cursor");
    assert_eq!(chosen, ["Delete"], "the menu did not take the arrows");
}

/// A focused text field built after an open menu: the menu has the keys, so
/// typing does not reach the field and Escape closes the menu without
/// unfocusing the field. After that the field types again.
#[test]
fn an_open_menu_keeps_the_keys_from_a_focused_field() {
    let mut ui = ui(Platform::Windows);
    let mut text = String::new();
    let field_focused = std::cell::Cell::new(false);
    let menu_open = std::cell::Cell::new(false);
    let rects = std::cell::Cell::new((Rect::default(), Rect::default()));
    let mut frame = |ui: &mut Ui, text: &mut String| {
        ui.begin_frame(info());
        let r = ui.button("Item");
        let open = ui.context_menu(&r, |ui| {
            ui.menu_item("Rename");
        });
        menu_open.set(open.is_some());
        let f = ui.text_input("name", text, "");
        field_focused.set(f.focused);
        rects.set((r.rect, f.response.rect));
        let _ = ui.end_frame();
    };
    frame(&mut ui, &mut text);
    frame(&mut ui, &mut text);
    let click = |ui: &mut Ui, at: Vec2, button: PointerButton, text: &mut String, frame: &mut dyn FnMut(&mut Ui, &mut String)| {
        ui.push(InputEvent::PointerMoved { pos: at });
        ui.push(InputEvent::PointerButton { button, pressed: true });
        frame(ui, text);
        ui.push(InputEvent::PointerButton { button, pressed: false });
        frame(ui, text);
    };
    let (item, field) = rects.get();
    click(&mut ui, field.center(), PointerButton::Primary, &mut text, &mut frame);
    assert!(field_focused.get(), "clicking the field did not focus it");
    click(&mut ui, item.center(), PointerButton::Secondary, &mut text, &mut frame);
    assert!(menu_open.get(), "the context menu did not open");

    ui.push(InputEvent::Text("x".into()));
    frame(&mut ui, &mut text);
    assert_eq!(text, "", "typing went to the field under an open menu");

    press(&mut ui, Key::Escape, false, |ui| frame(ui, &mut text));
    assert!(!menu_open.get(), "Escape did not close the menu");
    assert!(field_focused.get(), "the menu's Escape also unfocused the field");

    ui.push(InputEvent::Text("y".into()));
    frame(&mut ui, &mut text);
    assert_eq!(text, "y", "the field did not get the keys back");
}

/// Shift with the arrows grows a selection from where it started, and a plain
/// arrow after it goes back to one row.
#[test]
fn shift_and_the_arrows_extend_a_selection() {
    for platform in ALL {
        let mut ui = ui(platform);
        let mut picked: std::collections::BTreeSet<usize> = Default::default();
        let mut frame = |ui: &mut Ui| {
            ui.begin_frame(info());
            let nav = ui.open_collection("rows", 8);
            if nav.moved {
                let kind = if nav.extend { SelectKind::Range } else { SelectKind::Replace };
                match ui.select(nav.id, nav.cursor, kind) {
                    Selection::Only(i) => {
                        picked.clear();
                        picked.insert(i);
                    }
                    Selection::Range(r) => {
                        picked.clear();
                        picked.extend(r);
                    }
                    Selection::Toggle(i) => {
                        picked.insert(i);
                    }
                }
            }
            for i in 0..8 {
                ui.selectable_keyed(i, "Row", picked.contains(&i));
            }
            ui.close_collection();
            let _ = ui.end_frame();
        };
        frame(&mut ui);
        press(&mut ui, Key::Tab, false, &mut frame);
        press(&mut ui, Key::ArrowDown, false, &mut frame); // row 1, alone
        press(&mut ui, Key::ArrowDown, true, &mut frame);
        press(&mut ui, Key::ArrowDown, true, &mut frame); // 1..=3
        assert_eq!(picked.iter().copied().collect::<Vec<_>>(), [1, 2, 3], "{platform:?}: Shift+Down did not extend");

        let frame2 = |ui: &mut Ui, picked: &mut std::collections::BTreeSet<usize>| {
            ui.begin_frame(info());
            let nav = ui.open_collection("rows", 8);
            if nav.moved {
                let kind = if nav.extend { SelectKind::Range } else { SelectKind::Replace };
                if let Selection::Range(r) = ui.select(nav.id, nav.cursor, kind) {
                    picked.clear();
                    picked.extend(r);
                } else {
                    picked.clear();
                    picked.insert(nav.cursor);
                }
            }
            ui.close_collection();
            let _ = ui.end_frame();
        };
        press(&mut ui, Key::ArrowUp, true, |ui| frame2(ui, &mut picked)); // shrinks to 1..=2
        assert_eq!(picked.iter().copied().collect::<Vec<_>>(), [1, 2], "{platform:?}: Shift+Up did not shrink from the anchor");
        press(&mut ui, Key::ArrowDown, false, |ui| frame2(ui, &mut picked));
        assert_eq!(picked.iter().copied().collect::<Vec<_>>(), [3], "{platform:?}: a plain arrow did not go back to one row");
    }
}

const PARTS: [&str; 8] = ["Axle", "bearing", "Bolt", "Bolster", "Bracket", "Bushing", "Clip", "Collar"];

/// A list of parts with type-ahead, through the real keymap: what the user
/// types arrives as text, exactly as a host delivers it.
struct Parts {
    ui: Ui,
    cursor: usize,
    activated: bool,
}

impl Parts {
    fn new() -> Self {
        let mut p = Parts { ui: ui(Platform::Windows), cursor: 0, activated: false };
        p.frame();
        press(&mut p.ui, Key::Tab, false, |_| {});
        p.frame();
        p
    }

    fn frame(&mut self) {
        self.ui.begin_frame(info());
        let mut nav = self.ui.open_collection("parts", PARTS.len());
        self.ui.type_ahead(&mut nav, PARTS.len(), |i| PARTS[i]);
        self.cursor = nav.cursor;
        self.activated |= nav.activated;
        for (i, p) in PARTS.iter().enumerate() {
            self.ui.selectable_keyed(i, p, nav.cursor == i);
        }
        self.ui.close_collection();
        let _ = self.ui.end_frame();
    }

    fn type_text(&mut self, t: &str) {
        self.ui.push(InputEvent::Text(t.into()));
        self.frame();
    }

    /// Let time pass without typing.
    fn wait(&mut self, seconds: f32) {
        let frames = (seconds * 60.0) as usize;
        for _ in 0..frames {
            self.frame();
        }
    }
}

#[test]
fn typing_jumps_to_the_row_that_starts_with_it() {
    let mut p = Parts::new();
    p.type_text("b");
    assert_eq!(PARTS[p.cursor], "bearing", "b went to {}", PARTS[p.cursor]);
    p.type_text("r");
    assert_eq!(PARTS[p.cursor], "Bracket", "br went to {}", PARTS[p.cursor]);
    p.type_text("a");
    assert_eq!(PARTS[p.cursor], "Bracket", "bra moved off a row that still fits");

    // Typing on from "bo" to "bol" stays on Bolt, which still fits, rather
    // than moving to the next row that does.
    let mut p = Parts::new();
    p.type_text("bo");
    assert_eq!(PARTS[p.cursor], "Bolt");
    p.type_text("l");
    assert_eq!(PARTS[p.cursor], "Bolt", "bol left Bolt for {}", PARTS[p.cursor]);
}

/// While a menu is open over the list, what is typed is the menu's, not a
/// search.
#[test]
fn an_open_menu_keeps_typing_from_the_list() {
    let mut ui = ui(Platform::Windows);
    let cursor = std::cell::Cell::new(0usize);
    let row0 = std::cell::Cell::new(Rect::default());
    let frame = |ui: &mut Ui| {
        ui.begin_frame(info());
        let mut nav = ui.open_collection("parts", PARTS.len());
        ui.type_ahead(&mut nav, PARTS.len(), |i| PARTS[i]);
        cursor.set(nav.cursor);
        for (i, p) in PARTS.iter().enumerate() {
            let r = ui.selectable_keyed(i, p, nav.cursor == i);
            if i == 0 {
                row0.set(r.rect);
                ui.context_menu(&r, |ui| {
                    ui.menu_item("Rename");
                });
            }
        }
        ui.close_collection();
        let _ = ui.end_frame();
    };
    frame(&mut ui);
    press(&mut ui, Key::Tab, false, frame);
    ui.push(InputEvent::PointerMoved { pos: row0.get().center() });
    ui.push(InputEvent::PointerButton { button: PointerButton::Secondary, pressed: true });
    frame(&mut ui);
    ui.push(InputEvent::PointerButton { button: PointerButton::Secondary, pressed: false });
    frame(&mut ui);
    ui.push(InputEvent::Text("c".into()));
    frame(&mut ui);
    assert_eq!(cursor.get(), 0, "typing under an open menu searched the list");
}

#[test]
fn the_same_letter_again_cycles() {
    let mut p = Parts::new();
    for want in ["bearing", "Bolt", "Bolster", "Bracket", "Bushing", "bearing"] {
        p.type_text("b");
        assert_eq!(PARTS[p.cursor], want, "cycling b landed on {}", PARTS[p.cursor]);
    }
}

#[test]
fn a_pause_starts_a_new_search() {
    let mut p = Parts::new();
    p.type_text("c");
    assert_eq!(PARTS[p.cursor], "Clip");
    p.wait(1.5);
    p.type_text("a");
    assert_eq!(PARTS[p.cursor], "Axle", "after a pause, 'a' was read as 'ca'");
    // Without a pause it would have been one search: "co".
    let mut p = Parts::new();
    p.type_text("c");
    p.wait(0.3);
    p.type_text("o");
    assert_eq!(PARTS[p.cursor], "Collar", "a short pause split the search");
}

#[test]
fn space_activates_at_rest_and_is_text_mid_search() {
    // At rest: Space presses the row.
    let mut p = Parts::new();
    p.type_text("cl");
    p.wait(1.5);
    press(&mut p.ui, Key::Space, false, |_| {});
    p.frame();
    assert!(p.activated, "Space at rest did not activate the row");
    assert_eq!(PARTS[p.cursor], "Clip");

    // Mid-search: the space is part of what is being typed, and presses
    // nothing.
    let mut p = Parts::new();
    p.type_text("cl");
    press(&mut p.ui, Key::Space, false, |_| {});
    p.frame();
    assert!(!p.activated, "a space typed mid-search also activated the row");
}

#[test]
fn no_match_leaves_the_cursor_where_it_is() {
    let mut p = Parts::new();
    p.type_text("bo");
    p.type_text("z");
    assert_eq!(PARTS[p.cursor], "Bolt", "a search that fits nothing moved the cursor");
}

/// A slider and a drag value, adjusted from the keyboard.
struct Controls {
    ui: Ui,
    level: f32,
    gain: f32,
    offset: f32,
}

impl Controls {
    fn new() -> Self {
        let mut c = Controls { ui: ui(Platform::Windows), level: 0.5, gain: 0.5, offset: 10.0 };
        c.frame();
        c
    }
    fn frame(&mut self) {
        self.ui.begin_frame(info());
        self.ui.slider("Level", &mut self.level, 0.0, 1.0);
        self.ui.slider_vertical("Gain", &mut self.gain, 0.0, 1.0, 100.0);
        self.ui.drag_value("Offset", &mut self.offset, 0.5);
        let _ = self.ui.end_frame();
    }
    fn key(&mut self, k: Key, shift: bool) {
        press(&mut self.ui, k, shift, |_| {});
        self.frame();
        self.frame();
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn the_arrows_adjust_a_focused_slider() {
    let mut c = Controls::new();
    c.key(Key::Tab, false);
    for _ in 0..3 {
        c.key(Key::ArrowRight, false);
    }
    assert!(close(c.level, 0.53), "three steps right gave {}", c.level);
    c.key(Key::ArrowDown, false);
    assert!(close(c.level, 0.52), "down did not step down: {}", c.level);
    c.key(Key::ArrowRight, true);
    assert!(close(c.level, 0.62), "Shift did not take a big step: {}", c.level);
    c.key(Key::PageDown, false);
    assert!(close(c.level, 0.52), "Page Down did not take ten steps down: {}", c.level);
    c.key(Key::Home, false);
    assert_eq!(c.level, 0.0, "Home did not go to the minimum");
    c.key(Key::ArrowLeft, false);
    assert_eq!(c.level, 0.0, "a step past the minimum left the range");
    c.key(Key::End, false);
    assert_eq!(c.level, 1.0, "End did not go to the maximum");
    assert_eq!(c.gain, 0.5, "keys for one slider moved another");

    // The vertical one: up is more.
    c.key(Key::Tab, false);
    c.key(Key::ArrowUp, false);
    assert!(close(c.gain, 0.51), "up did not raise the vertical slider: {}", c.gain);

    // A drag value steps by its speed, and an unbounded one has no ends.
    c.key(Key::Tab, false);
    c.key(Key::ArrowRight, false);
    assert!(close(c.offset, 10.5), "the drag value did not step by its speed: {}", c.offset);
    c.key(Key::Home, false);
    assert!(close(c.offset, 10.5), "Home moved an unbounded drag value to {}", c.offset);
}

#[test]
fn the_arrows_adjust_a_focused_colour_picker() {
    let mut ui = ui(Platform::Windows);
    let mut color = Color::rgba(1.0, 0.5, 0.5, 1.0); // s 0.5, v 1, h 0
    let mut finished = 0;
    let mut frame = |ui: &mut Ui, color: &mut Color| {
        ui.begin_frame(info());
        ui.container(Layout::column().width(Size::Fixed(240.0)).height(Size::Fit), Frame::none(), |ui| {
            finished += ui.color_picker("c", color).finished as usize;
        });
        let _ = ui.end_frame();
    };
    frame(&mut ui, &mut color);
    let mut key = |ui: &mut Ui, color: &mut Color, k: Key| {
        press(ui, k, false, |_| {});
        frame(ui, color);
        frame(ui, color);
    };
    key(&mut ui, &mut color, Key::Tab); // the square
    key(&mut ui, &mut color, Key::ArrowLeft); // s 0.49
    key(&mut ui, &mut color, Key::ArrowDown); // v 0.99
    let (r, g) = (color.r, color.g);
    assert!(close(r, 0.99) && close(g, 0.99 * 0.51), "the square did not step s and v: {color:?}");

    key(&mut ui, &mut color, Key::Tab); // the hue strip
    let before = color;
    key(&mut ui, &mut color, Key::ArrowRight); // one degree
    assert!(color.g > before.g, "one degree of hue changed nothing: {before:?} -> {color:?}");
    assert!(color.g - before.g < 0.02, "a hue step was more than a degree: {before:?} -> {color:?}");

    key(&mut ui, &mut color, Key::Tab); // alpha
    key(&mut ui, &mut color, Key::ArrowLeft);
    assert!(close(color.a, 0.99), "alpha did not step: {}", color.a);
    assert_eq!(finished, 4, "each keyboard step is one edit for undo");
}
