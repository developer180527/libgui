//! Resizing the dense editor, one frame per pixel, the way dragging a window
//! corner does it.
//!
//! The thing being guarded is that a resize is *ordinary*. Every panel's rect
//! changes, so nothing can be replayed from the subtree cache — but that must
//! not turn into re-shaping text or re-rasterising glyphs, which is what makes
//! a resize feel like the UI is catching up with the window instead of moving
//! with it. Counts, not milliseconds, so it says the same on every machine.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn info(w: f32, h: f32) -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(w, h), scale: 2.0, dt: 1.0 / 60.0 }
}

#[test]
fn dragging_the_corner_stays_within_budget() {
    let mut ui = Ui::new(libgui_solaris::theme(), FONT).expect("font");
    ui.reserve(8_000);
    ui.audit = true;
    let mut app = libgui_solaris::App::default();

    // Settle at the starting size, so the caches are warm and the measurement
    // is of resizing rather than of starting up.
    for _ in 0..8 {
        ui.begin_frame(info(1800.0, 1000.0));
        app.ui(&mut ui);
        drop(ui.end_frame());
    }

    let mut worst = FrameCostPeak::default();
    for i in 0..300 {
        ui.begin_frame(info(1800.0 - i as f32, 1000.0 - i as f32 * 0.5));
        app.ui(&mut ui);
        drop(ui.end_frame());
        worst.take(&ui.frame_cost());
    }

    // Text is laid out once per string per size, not once per frame: if a
    // resize reshaped its labels these would climb with every pixel dragged.
    assert_eq!(worst.text_shaped, 0, "a resize reshaped text");
    assert_eq!(worst.glyphs_rasterized, 0, "a resize re-rasterised glyphs");
    assert!(worst.nodes < 1_200, "node count grew while resizing: {}", worst.nodes);
    assert!(worst.instances < 9_000, "instance count grew while resizing: {}", worst.instances);

    // And it must go quiet again once the new size has been built, or the host
    // never gets to idle after a drag.
    let settled = info(1500.0, 850.0);
    for _ in 0..6 {
        ui.begin_frame(settled);
        app.ui(&mut ui);
        drop(ui.end_frame());
    }
    assert!(!ui.needs_frame_for(&settled, 1.0), "the editor never settled after the resize");
}

/// The peak of each counter across a run of frames.
#[derive(Default)]
struct FrameCostPeak {
    nodes: usize,
    instances: usize,
    text_shaped: u32,
    glyphs_rasterized: u32,
}

impl FrameCostPeak {
    fn take(&mut self, c: &libgui::testing::FrameCost) {
        self.nodes = self.nodes.max(c.nodes);
        self.instances = self.instances.max(c.instances);
        self.text_shaped = self.text_shaped.max(c.text_shaped);
        self.glyphs_rasterized = self.glyphs_rasterized.max(c.glyphs_rasterized);
    }
}
