//! [`Ui::enabled`]: widgets that cannot be used, and look it.
//!
//! A command-enablement model — a CAD ribbon that greys out what does not
//! apply to the current selection, a panel whose fields are dead until
//! something is picked — was not expressible: only menu items had a disabled
//! state, so a greyed-out button meant re-implementing the button.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const INFO: FrameInfo = FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 };

struct World {
    ui: Ui,
    on: bool,
    /// The response of the button inside the scope.
    resp: Option<Response>,
    /// Alpha of every quad drawn this frame.
    alphas: Vec<f32>,
}

impl World {
    fn new(on: bool) -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let mut b = KeyBindings::new();
        b.bind(Shortcut::plain(Key::Tab), UiAction::FocusNext);
        ui.set_key_bindings(b);
        let mut w = Self { ui, on, resp: None, alphas: Vec::new() };
        w.frame();
        w.frame();
        w
    }

    fn frame(&mut self) {
        self.ui.begin_frame(INFO);
        let on = self.on;
        let mut resp = None;
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        self.ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            ui.button("Before");
            ui.enabled(on, |ui| {
                resp = Some(ui.button("Join"));
            });
            ui.button("After");
        });
        self.resp = resp;
        let out = self.ui.end_frame();
        self.alphas = out.draw.instances.iter().map(|i| i.color[3]).collect();
        let _ = out;
    }

    fn resp(&self) -> Response {
        self.resp.expect("no response")
    }

    fn hover_button(&mut self) {
        let at = self.resp().rect.center();
        self.ui.push(InputEvent::PointerMoved { pos: at });
        self.frame();
    }

    fn click_button(&mut self) {
        self.hover_button();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        self.frame();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        self.frame();
    }
}

/// **The point.** A disabled button does not respond to the pointer at all.
#[test]
fn a_disabled_widget_cannot_be_clicked_or_hovered() {
    let mut w = World::new(false);
    w.hover_button();
    assert!(!w.resp().hovered, "a disabled button reported a hover");
    w.click_button();
    assert!(!w.resp().clicked, "a disabled button reported a click");
    assert!(!w.resp().pressed);
    assert!(!w.resp().active);

    // The same button, enabled, does all three.
    let mut w = World::new(true);
    w.hover_button();
    assert!(w.resp().hovered, "an enabled button did not hover");
    w.click_button();
    assert!(w.resp().clicked, "an enabled button did not click");
}

/// It still has a rect, so the layout is identical either way: enabling a
/// command must not move the buttons beside it.
#[test]
fn disabling_does_not_change_the_layout() {
    let off = World::new(false).resp().rect;
    let on = World::new(true).resp().rect;
    assert_eq!(off, on, "a disabled button was laid out differently");
}

/// And it looks disabled: everything it paints is faded.
#[test]
fn a_disabled_widget_is_drawn_faded() {
    let mut off = World::new(false);
    off.frame();
    let mut on = World::new(true);
    on.frame();

    let faded = off.ui.theme.metrics.disabled_alpha;
    assert!(faded < 1.0);
    // The same number of quads either way — nothing is skipped, only dimmed.
    assert_eq!(off.alphas.len(), on.alphas.len(), "disabling changed what was drawn, not how");
    assert!(
        off.alphas.iter().any(|a| *a < 1.0 && *a > 0.0),
        "nothing was faded: {:?}",
        off.alphas
    );
    // Every quad the enabled pass drew opaque, the disabled pass drew at the
    // theme's alpha.
    let pairs = off.alphas.iter().zip(on.alphas.iter());
    let dimmed = pairs.filter(|(o, n)| **n > 0.99 && (**o - faded).abs() < 1e-3).count();
    assert!(dimmed > 0, "no quad was dimmed to exactly the theme's alpha");
}

/// The fade reaches an app's **own** drawing, because it is applied in the draw
/// list rather than by each widget. A CAD tool's custom controls grey out with
/// the rest without knowing they can.
#[test]
fn a_custom_widget_fades_without_knowing_how() {
    fn alphas(ui: &mut Ui, on: bool) -> Vec<f32> {
        ui.begin_frame(INFO);
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            ui.enabled(on, |ui| {
                let id = ui.make_id("custom");
                ui.add_leaf(id, Layout::leaf(Size::Fixed(50.0), Size::Fixed(20.0)), Vec2::ZERO, true, |p, r| {
                    // An app drawing its own thing, with no idea a scope exists.
                    p.rect(r, Color::WHITE, 0.0);
                });
            });
        });
        ui.end_frame().draw.instances.iter().map(|i| i.color[3]).collect()
    }
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let faded = ui.theme.metrics.disabled_alpha;
    alphas(&mut ui, true);
    let on = alphas(&mut ui, true);
    let off = alphas(&mut ui, false);
    assert_eq!(on, vec![1.0], "the custom rect was not drawn opaque when enabled");
    assert_eq!(off.len(), 1);
    assert!((off[0] - faded).abs() < 1e-3, "the custom rect did not fade: {off:?}");
}

/// Disabling nests one way. A group switched off has switched off everything
/// in it, and a child asking to be enabled is a bug rather than an intent.
#[test]
fn enabling_inside_a_disabled_scope_does_not_re_enable() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut inner = true;
    for _ in 0..2 {
        ui.begin_frame(INFO);
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            ui.enabled(false, |ui| {
                ui.enabled(true, |ui| {
                    inner = ui.is_enabled();
                });
            });
        });
        let _ = ui.end_frame();
    }
    assert!(!inner, "a nested enabled(true) re-enabled a disabled group");
}

/// The scope restores what it found, so widgets after it are live again.
#[test]
fn the_scope_puts_things_back() {
    let mut w = World::new(false);
    w.frame();
    assert!(w.ui.is_enabled(), "the scope leaked past its own closure");

    // The button *after* the disabled group still works.
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut after_hovered = false;
    let mut rect = Rect::default();
    for i in 0..4 {
        ui.begin_frame(INFO);
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
            ui.enabled(false, |ui| {
                ui.button("dead");
            });
            let r = ui.button("live");
            rect = r.rect;
            after_hovered = r.hovered;
        });
        let _ = ui.end_frame();
        if i == 1 {
            ui.push(InputEvent::PointerMoved { pos: rect.center() });
        }
    }
    assert!(after_hovered, "a widget after a disabled scope was dead too");
}

/// Keyboard focus skips a disabled widget, and leaves one that switches off
/// while it has focus — a ring that stayed on a dead control would trap Tab.
#[test]
fn focus_skips_a_disabled_widget_and_leaves_it_when_it_switches_off() {
    // Tab twice: past "Before", and the disabled "Join" must not be a stop.
    let mut w = World::new(false);
    for _ in 0..2 {
        w.ui.push(InputEvent::Key { key: Key::Tab, pressed: true, repeat: false });
        w.frame();
        w.ui.push(InputEvent::Key { key: Key::Tab, pressed: false, repeat: false });
        w.frame();
    }
    assert!(!w.resp().focused, "focus landed on a disabled widget");

    // Now focus it while enabled, then switch the scope off underneath it.
    let mut w = World::new(true);
    let mut guard = 0;
    while !w.resp().focused && guard < 8 {
        w.ui.push(InputEvent::Key { key: Key::Tab, pressed: true, repeat: false });
        w.frame();
        w.ui.push(InputEvent::Key { key: Key::Tab, pressed: false, repeat: false });
        w.frame();
        guard += 1;
    }
    assert!(w.resp().focused, "could not focus the button to begin with");
    w.on = false;
    w.frame();
    w.frame();
    assert!(!w.resp().focused, "focus stayed on a widget that switched off");
}
