//! Renders the editor to a PNG with the CPU backend, so the layout can be
//! looked at without a window — and so a change to it is visible in a diff.

use libgui::*;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const W: u32 = 2000;
const H: u32 = 1087;

#[test]
fn render_the_editor() {
    let scale = 1.0;
    let mut ui = Ui::new(libgui_solaris::theme(), FONT).expect("font");
    ui.reserve(8_000);
    let mut app = libgui_solaris::App::default();
    let info = FrameInfo {
        screen_size: Vec2::new(W as f32 / scale, H as f32 / scale),
        scale,
        dt: 1.0 / 60.0,
    };
    // A few frames, so everything that reads last frame's geometry has it.
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

    // And a guard while we are here: this is about as much UI as a tool puts
    // on one screen, and it has to stay cheap. Counts, not milliseconds, so it
    // says the same thing on every machine.
    libgui::testing::Budget::steady(1_200).instances(9_000).assert(&cost);
    assert!(cost.nodes > 500, "the editor did not build: {cost:?}");
}
