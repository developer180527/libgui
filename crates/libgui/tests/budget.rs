//! The public budget API, used the way an app is meant to use it — and proved
//! to actually catch each mistake it claims to catch.

use libgui::testing::{steady_frame, Budget, FrameCost};
use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(600.0, 400.0), scale: 2.0, dt: 1.0 / 60.0 }
}

/// A panel written the way the README says to write one.
fn good(ui: &mut Ui, names: &[String]) {
    ui.heading("Outliner");
    ui.virtual_list("objects", names.len(), 24.0, |ui, i| {
        ui.with_key(i, |ui| {
            let _ = ui.selectable(&names[i], false);
        });
    });
}

/// The same panel written the way it actually gets written first: every row
/// built, no keys, and a fresh string each frame.
fn bad(ui: &mut Ui, names: &[String], frame: u32) {
    ui.heading("Outliner");
    ui.scroll_area("objects", |ui| {
        for name in names {
            let _ = ui.selectable(name, false);
        }
        ui.label(&format!("{} objects, frame {frame}", names.len()));
    });
}

#[test]
fn a_virtualised_keyed_panel_keeps_a_tight_budget() {
    let names: Vec<String> = (0..10_000).map(|i| format!("Object {}", i % 40)).collect();
    let mut ui = ui();
    let cost = steady_frame(&mut ui, info(), |ui| good(ui, &names));

    // 10k rows, and the frame is the size of the window. A few offscreen
    // nodes are the list's overscan doing its job.
    Budget::steady(80).instances(200).offscreen_nodes(16).assert(&cost);
    assert!(cost.nodes < 80, "{cost:?}");
}

#[test]
fn the_budget_catches_each_mistake_it_claims_to() {
    let names: Vec<String> = (0..2_000).map(|i| format!("Object {}", i % 40)).collect();
    let mut ui = ui();
    let mut frame = 0;
    let cost = steady_frame(&mut ui, info(), |ui| {
        frame += 1;
        bad(ui, &names, frame)
    });

    let over = Budget::steady(80).offscreen_nodes(16).check(&cost);
    let joined = over.join("\n");
    assert!(joined.contains("nodes"), "the unvirtualised list was not flagged:\n{joined}");
    assert!(joined.contains("offscreen_nodes"), "rows built off screen were not flagged:\n{joined}");
    assert!(joined.contains("unkeyed_duplicates"), "the missing with_key was not flagged:\n{joined}");
    assert!(joined.contains("text_shaped"), "the per-frame string was not flagged:\n{joined}");

    // Worst first, so the message leads with the thing to fix.
    assert!(over[0].starts_with("text_shaped") || over[0].starts_with("offscreen_nodes"), "{over:?}");
}

#[test]
fn a_budget_with_nothing_set_passes_anything() {
    assert!(Budget::new().check(&FrameCost { nodes: 1 << 20, ..FrameCost::default() }).is_empty());
}

#[test]
fn keys_and_virtualisation_are_what_make_the_difference() {
    let names: Vec<String> = (0..2_000).map(|i| format!("Object {}", i % 40)).collect();
    let mut a = ui();
    let mut b = ui();
    let mut frame = 0;
    let fast = steady_frame(&mut a, info(), |ui| good(ui, &names));
    let slow = steady_frame(&mut b, info(), |ui| {
        frame += 1;
        bad(ui, &names, frame)
    });
    assert!(slow.nodes > fast.nodes * 20, "fast {fast:?}\nslow {slow:?}");
    assert_eq!(fast.unkeyed_duplicates, 0);
    assert!(slow.unkeyed_duplicates > 1_000, "slow {slow:?}");
    assert_eq!(fast.glyphs_rasterized, 0, "a steady frame rasterised glyphs: {fast:?}");
    assert_eq!(fast.text_shaped, 0, "a steady frame re-shaped text: {fast:?}");
    assert!(slow.text_shaped > 0, "the string rebuilt every frame was not noticed: {slow:?}");
}
