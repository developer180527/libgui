//! Renders the demo with the CPU backend in each theme — and once with a
//! swatch's picker open and the theme rebuilt around a picked accent — so the
//! layout can be looked at without a window.

use libgui::*;
use libgui_colors::Colors;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../../assets/Inter.ttf");
const SIZE: Vec2 = Vec2::new(1100.0, 780.0);

/// Settle, optionally click where `click` says (asked after settling, in real
/// app frames), settle again, and write the picture.
fn render(app: &mut Colors, ui: &mut Ui, scale: f32, name: &str, click: impl FnOnce(&Ui) -> Option<Vec2>) {
    let info = FrameInfo { screen_size: SIZE, scale, dt: 1.0 / 60.0 };
    for _ in 0..4 {
        ui.begin_frame(info);
        app.ui(ui);
        drop(ui.end_frame());
    }
    if let Some(at) = click(ui) {
        ui.push(InputEvent::PointerMoved { pos: at });
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        ui.begin_frame(info);
        app.ui(ui);
        drop(ui.end_frame());
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    }
    for _ in 0..30 {
        ui.begin_frame(info);
        app.ui(ui);
        drop(ui.end_frame());
    }
    ui.begin_frame(info);
    app.ui(ui);
    let out = ui.end_frame();
    let (w, h) = ((SIZE.x * scale) as u32, (SIZE.y * scale) as u32);
    let img = SoftRenderer::new().render_to_image(&out, w, h);
    drop(out);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    let file = std::fs::File::create(&path).expect("create");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("header").write_image_data(&img.data).expect("write");
    println!("{}", path.display());
}

#[test]
fn render_the_demo_in_every_theme() {
    for (theme, name) in [(0, "colors-dark.png"), (1, "colors-midnight.png"), (2, "colors-light.png")] {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let mut app = Colors::default();
        app.theme = theme;
        // A few finished edits, so the recent row has something in it.
        app.recent = vec![app.accent, app.layers[1], app.layers[3], app.compact];
        render(&mut app, &mut ui, 1.5, name, |_| None);
    }
}

#[test]
fn render_a_swatch_open_and_a_picked_accent() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut app = Colors::default();
    app.accent = Color::rgba(0.95, 0.35, 0.55, 1.0);
    app.accent_theme = true;
    app.recent = vec![app.accent, app.layers[2]];
    render(&mut app, &mut ui, 1.5, "colors-accent.png", |ui| {
        // Open the second layer's swatch, at the start of its row.
        let row = ui.rect_of(Id::new(("layer", 1usize))).expect("no layer row");
        Some(Vec2::new(row.x + 10.0, row.y + row.h * 0.5))
    });
    assert!(ui.any_popup_open(), "the swatch's picker did not open for the picture");
}
