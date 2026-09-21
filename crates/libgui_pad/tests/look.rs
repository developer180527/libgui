//! Renders Pad to a PNG with the CPU backend, so the layout can be looked at
//! without a window — and so a change to it shows up in a diff.

use libgui::*;
use libgui_keymap::Platform;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const W: u32 = 1320;
const H: u32 = 900;

fn render(pad: &mut libgui_pad::Pad, name: &str) -> testing::FrameCost {
    let mut ui = Ui::new(libgui_pad::theme(pad.paper), FONT).expect("font");
    libgui_keymap::Keymap::<u8>::for_current_platform().install(&mut ui);
    ui.reserve(4_000);
    let info = FrameInfo { screen_size: Vec2::new(W as f32, H as f32), scale: 1.0, dt: 1.0 / 60.0 };
    // A few frames, so everything that reads last frame's geometry has it.
    for _ in 0..5 {
        ui.begin_frame(info);
        pad.ui(&mut ui);
        let _ = ui.end_frame();
    }
    ui.begin_frame(info);
    pad.ui(&mut ui);
    let out = ui.end_frame();
    let img = SoftRenderer::new().render_to_image(&out, W, H);
    drop(out);

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    let file = std::fs::File::create(&path).expect("create");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("header").write_image_data(&img.data).expect("write");
    let cost = ui.frame_cost();
    println!("{} — {cost:?}", path.display());
    cost
}

#[test]
fn render_the_editor() {
    // The keymap is pinned, so the chords the menus and the status bar spell
    // are the same picture on every platform.
    let mut pad = libgui_pad::Pad::new(Platform::Mac);
    pad.find = "undo".into();
    let cost = render(&mut pad, "pad.png");

    // A document editor is the light end of what libgui is asked to carry, and
    // it has to stay there. Counts, not milliseconds, so it says the same
    // thing on every machine.
    libgui::testing::Budget::steady(600).instances(4_000).assert(&cost);
    assert!(cost.nodes > 50, "the editor did not build: {cost:?}");
}

#[test]
fn render_the_light_chrome() {
    let mut pad = libgui_pad::Pad::new(Platform::Mac);
    pad.paper = true;
    pad.side_tab = 1;
    render(&mut pad, "pad-light.png");
}
