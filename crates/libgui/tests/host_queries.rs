//! What a host asks of a `Ui` around its frames: how much UI there was,
//! whether an output is real, what is under a point.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(800.0, 600.0), scale: 1.0, dt: 1.0 / 60.0 }
}

fn editor(ui: &mut Ui) {
    ui.cached("editor", 0u32, |ui| {
        for i in 0..40 {
            ui.label(&format!("line {i}"));
        }
    });
}

/// A modal open over a cached editor: the editor replays, so it is not
/// *built* — but it is still there, and the counts say so.
#[test]
fn node_counts_include_replayed_subtrees_and_split_out_layers() {
    let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
    let mut cost = None;
    for _ in 0..4 {
        ui.begin_frame(info());
        editor(&mut ui);
        ui.modal("confirm", "Discard?", &ModalOptions::default(), |ui| {
            ui.label("Unsaved changes will be lost.");
        });
        drop(ui.end_frame());
        cost = Some(ui.frame_cost());
    }
    let replayed = cost.unwrap();

    let mut fresh = Ui::new(Theme::dark(), FONT).unwrap();
    fresh.begin_frame(info());
    editor(&mut fresh);
    fresh.modal("confirm", "Discard?", &ModalOptions::default(), |ui| {
        ui.label("Unsaved changes will be lost.");
    });
    drop(fresh.end_frame());
    let built = fresh.frame_cost();

    assert!(replayed.replayed_nodes > 0, "the editor never replayed: {replayed:?}");
    assert!(replayed.nodes < built.nodes, "a replay built as much as a build");
    assert_eq!(replayed.described_nodes(), built.described_nodes(), "a replay is not the same UI");
    assert_eq!(built.replayed_nodes, 0);
    assert!(built.layer_nodes > 0 && built.layer_nodes < built.nodes, "{built:?}");
    assert_eq!(replayed.layer_nodes, built.layer_nodes, "the modal is the same either way");
    assert!(built.main_nodes() > 40, "the editor's lines are in the main tree");
}

/// Only `end_frame` makes an output a host should apply; a default one is
/// recognisably not one.
#[test]
fn only_end_frame_output_is_built() {
    assert!(!PlatformOutput::default().built);
    let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
    ui.begin_frame(info());
    ui.label("hi");
    assert!(ui.end_frame().platform.built);
}

/// A button is hit, the space beside it is not, whatever the pointer does.
#[test]
fn hit_test_finds_widgets_and_not_empty_space() {
    let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
    let mut id = None;
    for _ in 0..2 {
        ui.begin_frame(info());
        let r = ui.button("Close");
        id = Some((r.id, r.rect));
        drop(ui.end_frame());
    }
    let (id, rect) = id.unwrap();
    assert_eq!(ui.hit_test(rect.center()), Some(id));
    assert_eq!(ui.hit_test(Vec2::new(700.0, 500.0)), None);
    assert!(!ui.wants_pointer(), "the pointer is nowhere; hit_test did not need it");
}
