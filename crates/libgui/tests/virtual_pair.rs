//! `open_virtual_list` builds exactly what `virtual_list_with` does: the
//! pair exists for callers that cannot pass a closure (the C API), and must
//! not be a second, drifting implementation.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn run(pair: bool) -> (Vec<[f32; 4]>, std::ops::Range<usize>) {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut built = 0..0;
    let mut rows = Vec::new();
    for frame in 0..60 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(300.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        // Asked to show row 5000 from the third frame on: the window moves.
        let opts = ListOptions { reveal: (frame >= 3).then_some(5000), ..ListOptions::new(24.0) };
        built = if pair {
            let r = ui.open_virtual_list("rows", 10_000, opts);
            for i in r.clone() {
                ui.open_virtual_row(i);
                ui.label(&format!("Row {i}"));
                ui.close_virtual_row();
            }
            ui.close_virtual_list();
            r
        } else {
            ui.virtual_list_with("rows", 10_000, opts, |ui, i| ui.label(&format!("Row {i}")))
        };
        let out = ui.end_frame();
        rows = out.draw.instances.iter().map(|i| i.rect).collect();
    }
    (rows, built)
}

#[test]
fn the_pair_builds_what_the_closure_does() {
    let (a, ra) = run(false);
    let (b, rb) = run(true);
    assert!(ra.start > 0, "the scroll did not move the window: {ra:?}");
    assert_eq!(ra, rb);
    assert_eq!(a, b);
}
