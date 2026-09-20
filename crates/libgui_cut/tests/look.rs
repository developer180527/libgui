//! Renders the editor to a PNG with the CPU backend, so the layout can be
//! looked at without a window — and so a change to it shows up in a diff.

use libgui::*;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const W: u32 = 2000;
const H: u32 = 1129;

#[test]
fn render_the_editor() {
    let scale = 1.0;
    let mut ui = Ui::new(libgui_cut::theme(), FONT).expect("font");
    ui.reserve(8_000);
    let mut app = libgui_cut::App::default();
    let info = FrameInfo { screen_size: Vec2::new(W as f32 / scale, H as f32 / scale), scale, dt: 1.0 / 60.0 };
    for _ in 0..5 {
        ui.begin_frame(info);
        app.ui(&mut ui);
        let _ = ui.end_frame();
    }
    ui.begin_frame(info);
    app.ui(&mut ui);
    let out = ui.end_frame();
    let img = SoftRenderer::new().render_to_image(&out, W, H);
    drop(out);
    let cost = ui.frame_cost();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editor.png");
    let file = std::fs::File::create(&path).expect("create");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("header").write_image_data(&img.data).expect("write");
    println!("{} — {cost:?}", path.display());
    assert!(cost.nodes > 200, "the editor did not build: {cost:?}");
}

/// The same editor after the user has zoomed in and scrolled: the ruler, the
/// clips and the scrollbars have to agree about where they are.
#[test]
fn render_scrolled_and_zoomed() {
    let mut ui = Ui::new(libgui_cut::theme(), FONT).expect("font");
    ui.reserve(8_000);
    let mut app = libgui_cut::App::default();
    let info = FrameInfo { screen_size: Vec2::new(W as f32, H as f32), scale: 1.0, dt: 1.0 / 60.0 };
    for _ in 0..4 {
        ui.begin_frame(info);
        app.ui(&mut ui);
        let _ = ui.end_frame();
    }
    app.ed.pps = 190.0;
    app.ed.scroll_x = 520.0;
    app.ed.scroll_y = 40.0;
    app.ed.playhead = 4.2;
    app.ed.selected_clip = Some(2);
    app.ed.playing = true;
    app.ed.tick(0.3);
    for _ in 0..2 {
        ui.begin_frame(info);
        app.ui(&mut ui);
        let _ = ui.end_frame();
    }
    ui.begin_frame(info);
    app.ui(&mut ui);
    let out = ui.end_frame();
    let img = SoftRenderer::new().render_to_image(&out, W, H);
    drop(out);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editor-scrolled.png");
    let file = std::fs::File::create(&path).expect("create");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("header").write_image_data(&img.data).expect("write");
    println!("{}", path.display());
}
