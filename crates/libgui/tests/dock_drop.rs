//! Moving a tab from one panel to another.
//!
//! The dock tears a tab off into a floating surface the moment it leaves its
//! tab bar, and hides that surface while the pointer is over a drop target. A
//! host that maps surfaces to OS windows leans on both halves of that: it can
//! skip building a window, a `Ui` with its own font atlas and a renderer with
//! its own pipelines for a surface that is never shown — and skip tearing all
//! of it down again on the drop, which is the part that lands between the drop
//! and the frame showing its result.
//!
//! So two things are pinned here: the drop resolves in a single frame, and a
//! tab that only moves between panels never asks for a visible window.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

struct V;
impl TabViewer for V {
    type Tab = &'static str;
    fn title(&self, t: &&'static str) -> String {
        t.to_string()
    }
    fn id(&self, t: &&'static str) -> u64 {
        t.as_ptr() as u64
    }
    fn ui(&mut self, ui: &mut Ui, t: &mut &'static str) {
        ui.label(t);
    }
}

const INFO: FrameInfo = FrameInfo { screen_size: Vec2::new(800.0, 600.0), scale: 1.0, dt: 1.0 / 60.0 };

/// Two panels side by side, the left one holding the tab we will move.
fn editor() -> (Ui, DockState<&'static str>) {
    let ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut dock = DockState::new();
    let left = dock.leaf(vec!["Scene Graph Tree"]);
    let right = dock.leaf(vec!["Scene View"]);
    let root = dock.split(Axis::X, 0.5, left, right);
    dock.set_root(SurfaceId::MAIN, root);
    dock.set_surface_frame(SurfaceId::MAIN, Vec2::ZERO, 1.0);
    (ui, dock)
}

fn frame(ui: &mut Ui, dock: &mut DockState<&'static str>) {
    ui.begin_frame(INFO);
    dock.show(ui, SurfaceId::MAIN, &mut V);
    drop(ui.end_frame());
}

/// Tabs per leaf, across every surface.
fn tabs(d: &DockState<&'static str>) -> Vec<usize> {
    let mut out = Vec::new();
    for s in d.surfaces() {
        let mut stack: Vec<&DockNode<&'static str>> = s.root.iter().collect();
        while let Some(n) = stack.pop() {
            match n {
                DockNode::Leaf(l) => out.push(l.tabs.len()),
                DockNode::Split(sp) => {
                    stack.push(&sp.first);
                    stack.push(&sp.second);
                }
            }
        }
    }
    out
}

fn drag_to(ui: &mut Ui, dock: &mut DockState<&'static str>, from: Vec2, to: Vec2, steps: usize) -> bool {
    let mut any_visible_float = false;
    for i in 1..=steps {
        let p = from + (to - from) * (i as f32 / steps as f32);
        ui.push(InputEvent::PointerMoved { pos: p });
        dock.set_pointer(p, true);
        dock.update();
        frame(ui, dock);
        any_visible_float |= dock.surfaces().iter().any(|s| s.id != SurfaceId::MAIN && s.visible);
    }
    any_visible_float
}

#[test]
fn a_tab_dropped_on_another_panel_lands_in_one_frame() {
    let (mut ui, mut dock) = editor();
    for _ in 0..5 {
        frame(&mut ui, &mut dock);
    }
    assert_eq!(tabs(&dock), vec![1, 1], "two panels, one tab each");

    // Press the left panel's tab.
    let grab = Vec2::new(60.0, 8.0);
    ui.push(InputEvent::PointerMoved { pos: grab });
    dock.set_pointer(grab, false);
    frame(&mut ui, &mut dock);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    dock.set_pointer(grab, true);
    dock.set_pointer_down(true);
    frame(&mut ui, &mut dock);

    // Out of the tab bar (which tears it off), then over the other panel's bar.
    let mut seen_visible = drag_to(&mut ui, &mut dock, grab, Vec2::new(110.0, 128.0), 10);
    seen_visible |= drag_to(&mut ui, &mut dock, Vec2::new(110.0, 128.0), Vec2::new(600.0, 8.0), 12);
    assert!(dock.drop_target().is_some(), "the pointer is over the other panel's tab bar");

    // The drop itself: one frame, and the floating surface is gone.
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    dock.set_pointer_down(false);
    dock.update();
    frame(&mut ui, &mut dock);

    assert_eq!(dock.surfaces().len(), 1, "the floating surface is gone on the drop frame");
    assert_eq!(tabs(&dock), vec![2], "both tabs are in one panel, one frame after the drop");

    // The premise the host optimises on: nothing was ever shown, so no OS
    // window had to be built for this and torn down again.
    assert!(
        !seen_visible,
        "a tab moved between panels must never ask for a visible window: a host \
         that builds one pays for it on the drop, not on the drag"
    );
}

/// The other half: a panel actually pulled out to the desktop *does* become
/// visible, or it could never be torn off at all.
#[test]
fn a_panel_dragged_clear_of_every_target_asks_for_a_window() {
    let (mut ui, mut dock) = editor();
    for _ in 0..5 {
        frame(&mut ui, &mut dock);
    }
    let grab = Vec2::new(60.0, 8.0);
    ui.push(InputEvent::PointerMoved { pos: grab });
    dock.set_pointer(grab, false);
    frame(&mut ui, &mut dock);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    dock.set_pointer(grab, true);
    dock.set_pointer_down(true);
    frame(&mut ui, &mut dock);
    drag_to(&mut ui, &mut dock, grab, Vec2::new(110.0, 128.0), 10);
    // Out past the window entirely: inside it, every point is some panel's
    // drop target, so "clear of everything" means off the window.
    drag_to(&mut ui, &mut dock, Vec2::new(110.0, 128.0), Vec2::new(1100.0, 900.0), 12);
    assert!(dock.drop_target().is_none(), "nothing to drop onto out here");

    // Let go over open space rather than over a panel.
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    dock.set_pointer_down(false);
    dock.update();
    frame(&mut ui, &mut dock);

    let floating: Vec<&Surface<&'static str>> =
        dock.surfaces().iter().filter(|s| s.id != SurfaceId::MAIN).collect();
    assert_eq!(floating.len(), 1, "the panel is now its own surface");
    assert!(floating[0].visible, "and it is visible, so the host gives it a window");
}
