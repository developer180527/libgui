//! Widgets squeezed narrower than their text: a label and a value never draw
//! over each other, and the value is the one that stays whole.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// The glyphs drawn: their rects and colours.
struct Drawn(Vec<([f32; 4], [f32; 4])>);

/// The glyph rects drawn in `color`.
fn glyphs(out: &Drawn, color: Color) -> Vec<[f32; 4]> {
    let c = color.to_array();
    out.0.iter().filter(|g| g.1 == c).map(|g| g.0).collect()
}

fn narrow(width: f32, mut build: impl FnMut(&mut Ui)) -> (Drawn, Theme) {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let theme = ui.theme.clone();
    let info = FrameInfo { screen_size: Vec2::new(width, 200.0), scale: 1.0, dt: 1.0 / 60.0 };
    ui.begin_frame(info);
    build(&mut ui);
    drop(ui.end_frame());
    ui.begin_frame(info);
    build(&mut ui);
    let out = ui.end_frame();
    let g = out.draw.instances.iter().filter(|i| i.params[3] == render_contract::PrimitiveKind::Glyph.code()).map(|i| (i.rect, i.color)).collect();
    (Drawn(g), theme)
}

/// A drag value 90 px wide with a long label: every digit of the value is
/// drawn, and no label glyph reaches into it.
#[test]
fn a_narrow_drag_value_keeps_its_value_whole() {
    let mut v = 1.0f32;
    let (out, t) = narrow(90.0, |ui| {
        ui.drag_value_range("Scale of the object", &mut v, 0.005, 0.3..=2.0);
    });
    let value = glyphs(&out, t.slider.value);
    let label = glyphs(&out, t.text_input.placeholder);
    assert_eq!(value.len(), "1.000".len(), "the value was not drawn whole");
    let left = value.iter().map(|r| r[0]).fold(f32::INFINITY, f32::min);
    for r in &label {
        assert!(r[0] + r[2] <= left, "a label glyph at x {}..{} runs into the value at {left}", r[0], r[0] + r[2]);
    }
}

/// With room for only part of it, the label is cut short rather than left
/// out; with room for none of it, it is left out.
#[test]
fn a_label_is_cut_short_then_dropped() {
    let count = |w: f32| {
        let mut v = 0.5f32;
        let (out, t) = narrow(w, |ui| {
            ui.slider("A rather long label", &mut v, 0.0, 1.0);
        });
        glyphs(&out, t.slider.label).len()
    };
    let wide = count(400.0);
    let mid = count(140.0);
    let tiny = count(40.0);
    assert!(mid > 0 && mid < wide, "the label was not cut short: {mid} glyphs against {wide}");
    assert_eq!(tiny, 0, "a label with no room was still drawn");
}

/// Wide enough for both, nothing changes: the whole label is drawn.
#[test]
fn a_wide_slider_draws_its_whole_label() {
    let mut v = 0.5f32;
    let (out, t) = narrow(400.0, |ui| {
        ui.slider("Volume", &mut v, 0.0, 1.0);
    });
    assert_eq!(glyphs(&out, t.slider.label).len(), "Volume".len());
}
