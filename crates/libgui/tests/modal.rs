//! Modal dialogs: nothing behind reachable, the keyboard kept inside, focus
//! given back, and Escape / Enter / a click outside reported rather than
//! acted on.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const SCREEN: Vec2 = Vec2::new(600.0, 400.0);

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

/// What one frame of the test app reported.
#[derive(Default, Debug)]
struct Seen {
    behind: Response,
    behind_field: Option<TextResponse>,
    one: Response,
    two: Response,
    modal: ModalResponse,
}

#[derive(Default)]
struct App {
    show: bool,
    field: String,
    name: String,
    nested: bool,
}

impl App {
    /// The app: a button, a field and a long list behind; when `show`, a
    /// dialog with two buttons and a field.
    fn frame(&mut self, ui: &mut Ui) -> Seen {
        let mut seen = Seen::default();
        ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt: 1.0 / 60.0 });
        seen.behind = ui.button("Behind");
        seen.behind_field = Some(ui.text_input("behind field", &mut self.field, ""));
        ui.container(Layout::column().width(Size::Grow(1.0)).height(Size::Fixed(150.0)), Frame::none(), |ui| {
            ui.scroll_area("list", |ui| {
                for i in 0..60 {
                    ui.label(&format!("Row {i}"));
                }
            });
        });
        if self.show {
            let name = &mut self.name;
            let nested = self.nested;
            let r = ui.modal("dialog", "Settings", &ModalOptions::default(), |ui| {
                let one = ui.button("One");
                let two = ui.button("Two");
                ui.text_input("name", name, "Name");
                if nested {
                    ui.modal("confirm", "Sure?", &ModalOptions::default(), |ui| {
                        let _ = ui.button("Yes");
                    });
                }
                (one, two)
            });
            (seen.one, seen.two) = r.inner;
            seen.modal = ModalResponse { inner: (), cancelled: r.cancelled, submitted: r.submitted, clicked_outside: r.clicked_outside, opened: r.opened };
        }
        drop(ui.end_frame());
        seen
    }

    fn run(&mut self, ui: &mut Ui, frames: usize) -> Seen {
        let mut s = Seen::default();
        for _ in 0..frames {
            s = self.frame(ui);
        }
        s
    }
}

fn click(ui: &mut Ui, app: &mut App, at: Vec2) -> Seen {
    ui.push(InputEvent::PointerMoved { pos: at });
    app.frame(ui);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    app.frame(ui);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    app.frame(ui)
}

fn act(ui: &mut Ui, app: &mut App, a: UiAction) -> Seen {
    ui.push(InputEvent::Action(a));
    app.frame(ui)
}

#[test]
fn nothing_behind_can_be_clicked_and_the_dialog_can() {
    let (mut ui, mut app) = (ui(), App::default());
    let s = app.run(&mut ui, 3);
    let behind = s.behind.rect;
    app.show = true;
    let s = app.run(&mut ui, 3);
    let s2 = click(&mut ui, &mut app, behind.center());
    assert!(!s2.behind.clicked && !s2.behind.hovered, "the button behind the dialog was reachable");
    assert!(s2.modal.clicked_outside, "a click on the dimmed window was not reported");
    assert!(app.show, "a click outside closed the dialog by itself");
    let s3 = click(&mut ui, &mut app, s.one.rect.center());
    assert!(s3.one.clicked, "the dialog's own button could not be clicked");
    assert!(!s3.modal.clicked_outside);
}

#[test]
fn the_wheel_does_not_scroll_what_is_behind() {
    let (mut ui, mut app) = (ui(), App::default());
    app.run(&mut ui, 3);
    let over_list = Vec2::new(100.0, 200.0);
    // Whether a wheel over the list moves what is drawn.
    let scrolls = |ui: &mut Ui, app: &mut App| -> bool {
        let snapshot = |ui: &mut Ui, app: &mut App| -> Vec<[f32; 4]> {
            ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt: 1.0 / 60.0 });
            app_build_only(ui, app);
            ui.end_frame().draw.instances.iter().map(|i| i.rect).collect()
        };
        let before = snapshot(ui, app);
        ui.push(InputEvent::PointerMoved { pos: over_list });
        ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -120.0), unit: WheelUnit::Pixel });
        app.run(ui, 30);
        before != snapshot(ui, app)
    };
    assert!(scrolls(&mut ui, &mut app), "the wheel does not scroll the list even with nothing over it");
    app.show = true;
    app.run(&mut ui, 3);
    assert!(!scrolls(&mut ui, &mut app), "the wheel scrolled the list behind the dialog");
}

/// One frame of the app's widgets, for comparing what is drawn.
fn app_build_only(ui: &mut Ui, app: &mut App) {
    let _ = ui.button("Behind");
    ui.text_input("behind field", &mut app.field, "");
    ui.container(Layout::column().width(Size::Grow(1.0)).height(Size::Fixed(150.0)), Frame::none(), |ui| {
        ui.scroll_area("list", |ui| {
            for i in 0..60 {
                ui.label(&format!("Row {i}"));
            }
        });
    });
    if app.show {
        ui.modal("dialog", "Settings", &ModalOptions::default(), |ui| {
            let _ = ui.button("One");
        });
    }
}

#[test]
fn a_field_behind_keeps_focus_but_takes_no_typing() {
    let (mut ui, mut app) = (ui(), App::default());
    let s = app.run(&mut ui, 3);
    let field = s.behind_field.unwrap().response.id;
    ui.set_focus(Some(field));
    app.run(&mut ui, 2);
    app.show = true;
    app.run(&mut ui, 3);
    // The dialog moved the keyboard into itself; put it back behind, as a
    // host bug or a stale id would, and type.
    ui.set_focus(Some(field));
    ui.push(InputEvent::Text("oops".into()));
    app.run(&mut ui, 2);
    assert_eq!(app.field, "", "typing reached a field behind the dialog");
}

#[test]
fn the_keyboard_moves_in_tab_stays_in_and_focus_comes_back() {
    let (mut ui, mut app) = (ui(), App::default());
    let s = app.run(&mut ui, 3);
    let behind = s.behind.id;
    ui.set_focus(Some(behind));
    app.run(&mut ui, 2);
    app.show = true;
    let s = app.run(&mut ui, 2);
    assert_eq!(ui.focused(), Some(s.one.id), "the keyboard did not move to the dialog's first control");
    // Three stops inside: One, Two, the name field. Tab round twice.
    let mut visited = Vec::new();
    for _ in 0..6 {
        act(&mut ui, &mut app, UiAction::FocusNext);
        visited.push(ui.focused());
    }
    assert!(visited.iter().all(|f| f.is_some() && *f != Some(behind)), "Tab left the dialog: {visited:?}");
    assert_eq!(visited[0], Some(s.two.id));
    assert_eq!(visited[0], visited[3], "Tab did not cycle within the dialog's three controls");
    app.show = false;
    app.run(&mut ui, 2);
    assert_eq!(ui.focused(), Some(behind), "focus did not come back to where it was before the dialog");
}

#[test]
fn escape_is_reported_even_from_a_field_and_enter_when_nothing_uses_it() {
    let (mut ui, mut app) = (ui(), App::default());
    app.show = true;
    let s = app.run(&mut ui, 3);
    let s1 = act(&mut ui, &mut app, UiAction::Cancel);
    assert!(s1.modal.cancelled, "Escape was not reported");
    assert!(app.show, "Escape closed the dialog by itself");
    // With the name field focused (two Tabs past One): the dialog still
    // hears it.
    for _ in 0..2 {
        act(&mut ui, &mut app, UiAction::FocusNext);
    }
    let s2 = act(&mut ui, &mut app, UiAction::Cancel);
    assert!(s2.modal.cancelled, "Escape from a focused field did not reach the dialog");
    // Enter on a focused button presses the button, not the dialog.
    ui.set_focus(Some(s.one.id));
    app.frame(&mut ui);
    let s3 = act(&mut ui, &mut app, UiAction::Submit);
    assert!(s3.one.clicked || ui.focused() == Some(s.one.id), "Enter on a focused button did nothing");
    assert!(!s3.modal.submitted, "Enter was both the button's and the dialog's");
    // With nothing focused, Enter is the dialog's default action.
    ui.set_focus(None);
    app.frame(&mut ui);
    let s4 = act(&mut ui, &mut app, UiAction::Submit);
    assert!(s4.modal.submitted, "an unused Enter was not reported as the dialog's");
}

#[test]
fn shortcuts_behind_wait_unless_let_through() {
    let shortcut = |ui: &mut Ui, opts: Option<ModalOptions>| -> bool {
        let mut fired = false;
        for k in 0..4 {
            if k == 3 {
                // A chord, as a real Save is: with anything focused (and the
                // dialog focuses its first control) a plain letter may be
                // typing, and libgui holds it back for that reason alone.
                ui.push(InputEvent::ModifiersChanged(Modifiers { ctrl: true, ..Modifiers::default() }));
                ui.push(InputEvent::Key { key: Key::S, pressed: true, repeat: false });
            }
            ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt: 1.0 / 60.0 });
            if let Some(o) = opts {
                ui.modal("d", "", &o, |ui| {
                    let _ = ui.button("x");
                });
            }
            fired |= ui.consume_shortcut(Shortcut::plain(Key::S).ctrl());
            drop(ui.end_frame());
        }
        fired
    };
    assert!(shortcut(&mut ui(), None), "the shortcut does not fire even with no dialog");
    assert!(!shortcut(&mut ui(), Some(ModalOptions::default())), "an app shortcut fired behind a dialog");
    assert!(shortcut(&mut ui(), Some(ModalOptions { shortcuts_behind: true, ..Default::default() })), "shortcuts_behind did not let it through");
}

#[test]
fn a_dialog_over_a_dialog_takes_the_keys_and_the_clicks() {
    let (mut ui, mut app) = (ui(), App::default());
    app.show = true;
    let s = app.run(&mut ui, 3);
    app.nested = true;
    app.run(&mut ui, 3);
    let s2 = click(&mut ui, &mut app, s.one.rect.center());
    assert!(!s2.one.clicked, "the dialog under the confirmation was clickable");
    let s3 = act(&mut ui, &mut app, UiAction::Cancel);
    assert!(!s3.modal.cancelled, "Escape went to the dialog underneath, not the one on top");
    assert!(ui.any_modal_open());
    app.nested = false;
    app.show = false;
    app.run(&mut ui, 2);
    assert!(!ui.any_modal_open());
}

#[test]
fn closing_a_modal_twice_is_refused() {
    let r = std::panic::catch_unwind(|| {
        let mut ui = ui();
        ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt: 1.0 / 60.0 });
        ui.close_modal();
    });
    assert!(r.is_err());
}

/// Escape with a combo open inside the dialog closes the combo, and only the
/// next Escape reaches the dialog: one level at a time, as menus back out.
#[test]
fn escape_closes_a_popup_inside_the_dialog_first() {
    let mut ui = ui();
    let mut choice = 0usize;
    let mut step = |ui: &mut Ui| -> (Response, ModalResponse<()>) {
        ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt: 1.0 / 60.0 });
        let mut combo = Response::default();
        let r = ui.modal("d", "Units", &ModalOptions::default(), |ui| {
            combo = ui.combo("Length", &mut choice, &["mm", "in", "m"]);
        });
        drop(ui.end_frame());
        (combo, ModalResponse { inner: (), ..r })
    };
    let mut combo = Response::default();
    for _ in 0..3 {
        combo = step(&mut ui).0;
    }
    ui.push(InputEvent::PointerMoved { pos: combo.rect.center() });
    step(&mut ui);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    step(&mut ui);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    step(&mut ui);
    step(&mut ui);
    assert!(ui.any_popup_open(), "the combo did not open inside the dialog");
    ui.push(InputEvent::Action(UiAction::Cancel));
    let (_, r) = step(&mut ui);
    assert!(!ui.any_popup_open(), "Escape did not close the combo");
    assert!(!r.cancelled, "Escape closed the combo and cancelled the dialog at once");
    step(&mut ui);
    ui.push(InputEvent::Action(UiAction::Cancel));
    let (_, r) = step(&mut ui);
    assert!(r.cancelled, "the second Escape did not reach the dialog");
}

/// Splitters, table grips and the dock's dividers hit-test *before* ordinary
/// widgets, whatever is painted over them — so being under a dialog is not
/// enough by itself. The dialog's scrim drops every hit beneath it.
#[test]
fn a_splitter_behind_cannot_be_dragged() {
    let mut ui = ui();
    let mut width = 200.0f32;
    let mut step = |ui: &mut Ui, show: bool| -> Response {
        ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt: 1.0 / 60.0 });
        let r = ui.row(|ui| {
            ui.container(Layout::column().width(Size::Fixed(width)).height(Size::Grow(1.0)), Frame::none(), |ui| ui.label("Side"));
            let r = ui.splitter("side", &mut width, SplitterOptions::vertical_rule(100.0, 400.0));
            ui.label("Main");
            r
        });
        if show {
            ui.modal("d", "Blocking", &ModalOptions::default(), |ui| ui.label("Nothing behind me moves"));
        }
        drop(ui.end_frame());
        r
    };
    let mut grip = Response::default();
    for _ in 0..3 {
        grip = step(&mut ui, true);
    }
    // The splitter sits at x = 200, left of the centred dialog.
    let at = grip.rect.center();
    ui.push(InputEvent::PointerMoved { pos: at });
    step(&mut ui, true);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    step(&mut ui, true);
    ui.push(InputEvent::PointerMoved { pos: at + Vec2::new(60.0, 0.0) });
    step(&mut ui, true);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    step(&mut ui, true);
    assert_eq!(width, 200.0, "a splitter behind the dialog was dragged");
}
