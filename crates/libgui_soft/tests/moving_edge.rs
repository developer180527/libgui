//! An edge that moves draws the same pixels as an edge at rest.
//!
//! Reported from a bgfx host: dragging the edge of a pane beside a 3D view,
//! the view's border "broke for a moment". Nothing was missing. The pane edge
//! followed the pointer by fractional pixels — a delta of 3.37 px — so the 1 px
//! border landed between pixels, and from frame to frame it alternated
//! between one crisp pixel and two half-strength ones. At 60 Hz that reads as
//! the line flickering. A `Grow` split of an odd width does the same during a
//! live resize.
//!
//! Layout now puts every edge on the physical pixel grid. These render the
//! moving edge through both the instanced path and the triangle path a bgfx
//! host uses, at every scale, and require the border pixel of every frame to
//! be the border pixel at rest.

use libgui::mesh::Mesh;
use libgui::*;
use libgui_soft::{SoftRenderer, Target, Texture};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const SCENE: [u8; 4] = [20, 30, 60, 255];

struct Rig {
    ui: Ui,
    soft: SoftRenderer,
    tex: TextureId,
    size: Vec2,
    scale: f32,
    right_w: f32,
    split: Rect,
    /// The right pane grows rather than taking `right_w`, so a resize splits
    /// the width between two growing panes.
    grow_split: bool,
}

impl Rig {
    fn new(scale: f32, grow_split: bool) -> Self {
        let mut soft = SoftRenderer::new();
        let tex = soft.register_texture(Texture { width: 4, height: 4, data: SCENE.repeat(16) });
        let ui = Ui::new(Theme::dark(), FONT).expect("font");
        Self { ui, soft, tex, size: Vec2::new(400.0, 200.0), scale, right_w: 120.0, split: Rect::default(), grow_split }
    }

    /// One frame, drawn both ways.
    fn frame(&mut self) -> (Target, Target) {
        self.ui.begin_frame(FrameInfo { screen_size: self.size, scale: self.scale, dt: 1.0 / 60.0 });
        let (tex, grow_split) = (self.tex, self.grow_split);
        let mut right_w = self.right_w;
        let mut split = self.split;
        self.ui.container(Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0)), Frame::none(), |ui| {
            let main = if grow_split { Size::Grow(2.0) } else { Size::Grow(1.0) };
            ui.container(Layout::column().width(main).height(Size::Grow(1.0)), Frame::none(), |ui| {
                ui.viewport("scene", tex, |_, _| {});
            });
            split = ui.splitter("right", &mut right_w, SplitterOptions::vertical_rule(40.0, 300.0).inverted()).rect;
            let w = if grow_split { Size::Grow(1.0) } else { Size::Fixed(right_w) };
            let fill = Frame { fill: Color::rgba(0.15, 0.15, 0.17, 1.0), ..Frame::none() };
            ui.container(Layout::column().width(w).height(Size::Grow(1.0)), fill, |ui| ui.label("Inspector"));
        });
        self.right_w = right_w;
        self.split = split;
        let out = self.ui.end_frame();
        let (w, h) = ((self.size.x * self.scale).round() as u32, (self.size.y * self.scale).round() as u32);
        let instanced = self.soft.render_to_image(&out, w, h);
        let mut mesh = Mesh::new();
        mesh.build(out.draw);
        let expanded = self.soft.render_mesh_to_image(&out, &mesh, w, h);
        (instanced, expanded)
    }
}

/// The first pixel right of the 3D view, along the middle row: its border.
fn border(t: &Target) -> [u8; 4] {
    let y = t.height / 2;
    let last_scene = (0..t.width).filter(|&x| t.pixel(x, y)[..3] == SCENE[..3]).max().expect("no scene on screen");
    t.pixel(last_scene + 1, y)
}

fn same(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(&b).all(|(a, b)| a.abs_diff(*b) <= 1)
}

fn check(scale: f32, rest: [u8; 4], moving: impl IntoIterator<Item = (Target, Target)>, what: &str) {
    for (i, (instanced, expanded)) in moving.into_iter().enumerate() {
        for (path, t) in [("instanced", &instanced), ("triangles", &expanded)] {
            let b = border(t);
            assert!(
                same(b, rest),
                "{what} at {scale}x, frame {i}, {path}: the border is {b:?} where at rest it is {rest:?} — \
                 the edge is between pixels"
            );
        }
    }
}

#[test]
fn a_dragged_edge_draws_its_border_as_it_does_at_rest() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let mut rig = Rig::new(scale, false);
        let mut rest = None;
        for _ in 0..4 {
            rest = Some(rig.frame().0);
        }
        let rest = border(&rest.unwrap());

        let c = rig.split.center();
        rig.ui.push(InputEvent::PointerMoved { pos: c });
        rig.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        let mut x = c.x;
        let frames: Vec<_> = (0..24)
            .map(|_| {
                x -= 3.37; // never a whole pixel
                rig.ui.push(InputEvent::PointerMoved { pos: Vec2::new(x, c.y) });
                rig.frame()
            })
            .collect();
        assert!(rig.right_w > 180.0, "the drag did not move the edge (right_w {})", rig.right_w);
        check(scale, rest, frames, "dragging the edge");
    }
}

#[test]
fn a_live_resize_draws_its_border_as_it_does_at_rest() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let mut rig = Rig::new(scale, true);
        let mut rest = None;
        for _ in 0..4 {
            rest = Some(rig.frame().0);
        }
        let rest = border(&rest.unwrap());
        let frames: Vec<_> = (0..24)
            .map(|i| {
                // A window edge dragged by the OS, one physical pixel at a
                // time, so the two-to-one split falls on a third of a pixel.
                rig.size.x = 400.0 + (i + 1) as f32 / scale;
                rig.frame()
            })
            .collect();
        check(scale, rest, frames, "resizing the window");
    }
}

/// The case as reported: a 3D view in a dock tab, and the dock's own split
/// handle dragged. The dock lays its panes out as `Grow(fraction)` nodes, so
/// a drag moves the edge by `delta / total` of the width — never whole
/// pixels — and this is the path a C host's dock takes.
#[test]
fn a_dragged_dock_split_draws_the_views_border_as_it_does_at_rest() {
    struct Panels(TextureId);
    impl TabViewer for Panels {
        type Tab = &'static str;
        fn title(&self, t: &&'static str) -> String {
            t.to_string()
        }
        fn id(&self, t: &&'static str) -> u64 {
            t.len() as u64
        }
        fn ui(&mut self, ui: &mut Ui, t: &mut &'static str) {
            if *t == "Scene" {
                ui.viewport("scene", self.0, |_, _| {});
            } else {
                ui.label("Properties");
            }
        }
    }
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let mut soft = SoftRenderer::new();
        let tex = soft.register_texture(Texture { width: 4, height: 4, data: SCENE.repeat(16) });
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let mut dock = DockState::new();
        let (a, b) = (dock.leaf(vec!["Scene"]), dock.leaf(vec!["Inspector"]));
        let root = dock.split(Axis::X, 0.62, a, b);
        dock.set_root(SurfaceId::MAIN, root);
        dock.set_surface_frame(SurfaceId::MAIN, Vec2::ZERO, 1.0);
        let split_id = match &dock.surfaces()[0].root {
            Some(DockNode::Split(s)) => s.id,
            _ => panic!("the root is not a split"),
        };
        let mut viewer = Panels(tex);
        let size = Vec2::new(500.0, 260.0);
        let (w, h) = ((size.x * scale).round() as u32, (size.y * scale).round() as u32);
        let mut frame = |ui: &mut Ui, dock: &mut DockState<&'static str>| {
            ui.begin_frame(FrameInfo { screen_size: size, scale, dt: 1.0 / 60.0 });
            dock.show(ui, SurfaceId::MAIN, &mut viewer);
            let out = ui.end_frame();
            let instanced = soft.render_to_image(&out, w, h);
            let mut mesh = Mesh::new();
            mesh.build(out.draw);
            (instanced, soft.render_mesh_to_image(&out, &mesh, w, h))
        };
        let mut rest = None;
        for _ in 0..4 {
            rest = Some(frame(&mut ui, &mut dock).0);
        }
        let rest = border(&rest.unwrap());
        let handle = ui.rect_of(Id::new(("dock_splitter", split_id))).expect("the dock's split handle was not laid out");
        let c = handle.center();
        ui.push(InputEvent::PointerMoved { pos: c });
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        let mut x = c.x;
        let frames: Vec<_> = (0..24)
            .map(|_| {
                x -= 3.37;
                ui.push(InputEvent::PointerMoved { pos: Vec2::new(x, c.y) });
                frame(&mut ui, &mut dock)
            })
            .collect();
        let moved = ui.rect_of(Id::new(("dock_splitter", split_id))).unwrap();
        assert!(c.x - moved.center().x > 40.0, "the drag did not move the dock's split ({:?} -> {:?})", handle, moved);
        check(scale, rest, frames, "dragging the dock's split");
    }
}
