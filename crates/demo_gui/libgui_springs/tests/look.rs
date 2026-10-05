//! Renders the demo with the CPU backend, caught mid-motion, so the layout can
//! be looked at without a window.

use libgui::*;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../../assets/Inter.ttf");

#[test]
fn render_the_demo_mid_motion() {
    let scale = 2.0;
    let size = Vec2::new(720.0, 760.0);
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut app = libgui_springs::Springs::default();
    let info = FrameInfo { screen_size: size, scale, dt: 1.0 / 60.0 };
    for _ in 0..4 {
        ui.begin_frame(info);
        app.ui(&mut ui);
        drop(ui.end_frame());
    }
    // Flip the switches, and catch them a tenth of a second in: the ease
    // nearly there, the springs at different points of their curves.
    app.on = true;
    for _ in 0..6 {
        ui.begin_frame(info);
        app.ui(&mut ui);
        drop(ui.end_frame());
    }
    ui.begin_frame(info);
    app.ui(&mut ui);
    let out = ui.end_frame();
    let (w, h) = ((size.x * scale) as u32, (size.y * scale) as u32);
    let img = SoftRenderer::new().render_to_image(&out, w, h);
    drop(out);

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("springs.png");
    let file = std::fs::File::create(&path).expect("create");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("header").write_image_data(&img.data).expect("write");
    println!("{}", path.display());
}
