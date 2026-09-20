//! A menu must open under the finger that opened it.
//!
//! This is a feel bug, and feel bugs hide from profilers. The frame behind a
//! menu bar costs a fraction of a millisecond and every budget test passes —
//! but if the menu waits for the *release*, it arrives however long the user
//! happened to hold the button, which is the one delay a person actually
//! notices. So the thing pinned here is not a cost, it is an ordering: the
//! popup exists on the press frame, before any release is sent.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 2.0, dt: 1.0 / 60.0 }
}

/// Builds a menu button and reports how many instances the frame drew.
fn frame(ui: &mut Ui) -> usize {
    ui.begin_frame(info());
    ui.menu_button("File", |ui| {
        for label in ["New", "Open", "Save"] {
            let _ = ui.menu_item(label);
        }
    });
    let out = ui.end_frame();
    let n = out.draw.instances.len();
    drop(out);
    n
}

#[test]
fn a_menu_opens_on_the_press_and_not_on_the_release() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");

    // Settle, so the button has a rect to be hovered against.
    frame(&mut ui);
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(20.0, 10.0) });
    frame(&mut ui);
    let closed = frame(&mut ui);
    assert!(!ui.any_popup_open(), "nothing is open before the press");

    // Press and hold. No release is sent from here on — that is the whole point.
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    frame(&mut ui);
    assert!(ui.any_popup_open(), "the menu is open on the press frame, while the button is still down");

    // Its size comes from that frame's measure, so the frame after it is drawn
    // in full — still with the button held, and with no new input at all.
    let held = frame(&mut ui);
    assert!(held > closed, "the menu is drawn while held ({closed} instances closed, {held} open)");

    // Releasing must not toggle it shut again: only a fresh press does that.
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    assert_eq!(frame(&mut ui), held, "the release that ended the opening click leaves it open");

    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    frame(&mut ui);
    assert!(!ui.any_popup_open(), "pressing the button again closes it");
}

/// The provisional first frame must ask to be drawn again, or a host that
/// honours `needs_frame` would leave the popup at zero height until some
/// unrelated input woke it up.
#[test]
fn opening_a_popup_requests_the_frame_that_sizes_it() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");

    frame(&mut ui);
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(20.0, 10.0) });
    frame(&mut ui);
    frame(&mut ui);

    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    frame(&mut ui);
    assert!(
        ui.needs_frame(1.0),
        "the frame that opened the popup is provisional and must request another"
    );
}

#[test]
fn a_combo_opens_on_the_press_too() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut selected = 0usize;

    let frame = |ui: &mut Ui, selected: &mut usize| {
        ui.begin_frame(info());
        ui.combo("Colorspace", selected, &["Linear", "sRGB", "ACES"]);
        drop(ui.end_frame());
    };

    frame(&mut ui, &mut selected);
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(40.0, 10.0) });
    frame(&mut ui, &mut selected);
    frame(&mut ui, &mut selected);
    assert!(!ui.any_popup_open(), "nothing is open before the press");

    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    frame(&mut ui, &mut selected);
    assert!(ui.any_popup_open(), "the list opens on the press, while the button is still down");
}
