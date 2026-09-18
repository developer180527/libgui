//! Interaction tests. The crate reports edits rather than applying them, so a
//! test is "drive the pointer, check the events".

use libgui::*;
use libgui_nodes::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

struct Doc {
    nodes: Vec<(NodeId, Vec2)>,
    links: Vec<Link>,
}

impl Doc {
    fn two() -> Self {
        Doc { nodes: vec![(1, Vec2::new(40.0, 40.0)), (2, Vec2::new(400.0, 40.0))], links: Vec::new() }
    }
    fn apply(&mut self, e: &GraphEvent) {
        match e {
            GraphEvent::NodeMoved { node, delta } => {
                if let Some(n) = self.nodes.iter_mut().find(|(id, _)| id == node) {
                    n.1 += *delta;
                }
            }
            GraphEvent::LinkCreated { link } => self.links.push(*link),
            GraphEvent::LinkRemoved { link } => self.links.retain(|l| l != link),
            _ => {}
        }
    }
}

struct Harness {
    ui: Ui,
    st: GraphState,
    style: GraphStyle,
    doc: Doc,
}

impl Harness {
    fn new() -> Self {
        let ui = Ui::new(Theme::dark(), FONT).unwrap();
        let style = GraphStyle::from_theme(&Theme::dark());
        Self { ui, st: GraphState::new(), style, doc: Doc::two() }
    }

    /// One frame; returns the events it produced (already applied to the doc).
    fn frame(&mut self) -> Vec<GraphEvent> {
        self.ui.begin_frame(FrameInfo::default());
        let nodes = self.doc.nodes.clone();
        let links = self.doc.links.clone();
        let (events, _) = graph(&mut self.ui, "g", &mut self.st, &self.style, |g| {
            for (id, pos) in &nodes {
                let cfg = NodeConfig::new("Node").inputs(&["In"]).outputs(&["Out"]);
                g.node(*id, *pos, &cfg, |_ui| {});
            }
            for l in &links {
                g.link(l.from, l.to);
            }
        });
        let _ = self.ui.end_frame();
        for e in &events {
            self.doc.apply(e);
        }
        events
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.frame();
        }
    }

    fn move_to(&mut self, p: Vec2) {
        self.ui.push(InputEvent::PointerMoved { pos: p });
    }
    fn press(&mut self) {
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    }
    fn release(&mut self) {
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    }

    /// Canvas point -> window point (the canvas fills the window).
    fn win(&self, p: Vec2) -> Vec2 {
        let v = &self.st.view;
        Vec2::new(p.x * v.zoom + v.pan.x, p.y * v.zoom + v.pan.y)
    }
}

/// Dragging a node's header moves it, and moves everything selected with it.
#[test]
fn dragging_a_header_moves_the_node_and_the_selection() {
    let mut h = Harness::new();
    h.settle();
    let start = h.doc.nodes[0].1;
    let header = h.win(Vec2::new(start.x + 60.0, start.y + 10.0));

    h.move_to(header);
    h.press();
    h.frame();
    h.move_to(Vec2::new(header.x + 50.0, header.y + 30.0));
    h.frame();
    h.release();
    h.frame();

    let moved = h.doc.nodes[0].1;
    assert!((moved.x - (start.x + 50.0)).abs() < 0.5, "node at {moved:?}, expected +50 in x");
    assert!(h.st.is_selected(1), "pressing a node did not select it");
    assert!(!h.st.is_selected(2));

    // Select both, then drag one: both move.
    h.st.selection.insert(2);
    h.settle();
    let (a0, b0) = (h.doc.nodes[0].1, h.doc.nodes[1].1);
    let header = h.win(Vec2::new(a0.x + 60.0, a0.y + 10.0));
    h.move_to(header);
    h.press();
    h.frame();
    h.move_to(Vec2::new(header.x + 20.0, header.y));
    h.frame();
    h.release();
    h.frame();
    assert!((h.doc.nodes[0].1.x - (a0.x + 20.0)).abs() < 0.5, "dragged node did not move");
    assert!((h.doc.nodes[1].1.x - (b0.x + 20.0)).abs() < 0.5, "the rest of the selection did not follow");
}

/// Dragging from an output to an input makes a link, and the end snaps to a
/// port it comes near rather than needing a pixel-perfect drop.
#[test]
fn dragging_between_ports_creates_a_link_and_snaps() {
    let mut h = Harness::new();
    h.settle();
    let out = h.st.view; // keep the view; positions below are canvas units
    let _ = out;

    // Output port of node 1, input port of node 2, from the style's geometry.
    let s = h.style;
    let (n1, n2) = (h.doc.nodes[0].1, h.doc.nodes[1].1);
    let out_pos = Vec2::new(n1.x + s.node_width, n1.y + s.header_height + s.port_row * 0.5);
    let in_pos = Vec2::new(n2.x, n2.y + s.header_height + s.port_row * 0.5);

    h.move_to(h.win(out_pos));
    h.press();
    h.frame();
    // Drop *near* the input, not on it: within the snap radius.
    let near = Vec2::new(in_pos.x - 10.0, in_pos.y - 6.0);
    h.move_to(h.win(near));
    h.frame();
    h.frame();
    h.release();
    let events = h.frame();

    let created: Vec<&GraphEvent> = events.iter().filter(|e| matches!(e, GraphEvent::LinkCreated { .. })).collect();
    assert_eq!(created.len(), 1, "expected one link, got {events:?}");
    assert_eq!(
        h.doc.links,
        vec![Link { from: PortId::output(1, 0), to: PortId::input(2, 0) }],
        "link connected the wrong ports"
    );
}

/// Dropping a dragged link on empty canvas makes nothing.
#[test]
fn dropping_a_link_on_nothing_creates_nothing() {
    let mut h = Harness::new();
    h.settle();
    let s = h.style;
    let n1 = h.doc.nodes[0].1;
    let out_pos = Vec2::new(n1.x + s.node_width, n1.y + s.header_height + s.port_row * 0.5);

    h.move_to(h.win(out_pos));
    h.press();
    h.frame();
    h.move_to(h.win(Vec2::new(250.0, 500.0)));
    h.frame();
    h.release();
    h.frame();
    assert!(h.doc.links.is_empty(), "a link was made out of nothing: {:?}", h.doc.links);
}

/// Dragging the input end of an existing link picks it up and re-routes it:
/// one removal, then one creation at the new port.
#[test]
fn dragging_an_input_end_reroutes_the_link() {
    let mut h = Harness::new();
    h.doc.nodes.push((3, Vec2::new(400.0, 300.0)));
    h.doc.links.push(Link { from: PortId::output(1, 0), to: PortId::input(2, 0) });
    h.settle();

    let s = h.style;
    let n2 = h.doc.nodes[1].1;
    let n3 = h.doc.nodes[2].1;
    let in2 = Vec2::new(n2.x, n2.y + s.header_height + s.port_row * 0.5);
    let in3 = Vec2::new(n3.x, n3.y + s.header_height + s.port_row * 0.5);

    // Grab the end sitting in node 2's input.
    h.move_to(h.win(in2));
    h.press();
    let picked = h.frame();
    assert!(
        picked.iter().any(|e| matches!(e, GraphEvent::LinkRemoved { .. })),
        "grabbing an input end did not detach the link: {picked:?}"
    );
    assert!(h.doc.links.is_empty(), "the link should be detached while dragging");

    // Drop it on node 3's input.
    h.move_to(h.win(in3));
    h.frame();
    h.frame();
    h.release();
    h.frame();
    assert_eq!(
        h.doc.links,
        vec![Link { from: PortId::output(1, 0), to: PortId::input(3, 0) }],
        "the link did not re-route to the new port"
    );
}

/// A link is pickable along its curve, not just by its bounding box.
#[test]
fn clicking_a_link_selects_it() {
    let mut h = Harness::new();
    h.doc.links.push(Link { from: PortId::output(1, 0), to: PortId::input(2, 0) });
    h.settle();

    let s = h.style;
    let (n1, n2) = (h.doc.nodes[0].1, h.doc.nodes[1].1);
    let a = Vec2::new(n1.x + s.node_width, n1.y + s.header_height + s.port_row * 0.5);
    let b = Vec2::new(n2.x, n2.y + s.header_height + s.port_row * 0.5);
    // The two ports are at the same height, so the curve's midpoint is too.
    let mid = Vec2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);

    h.move_to(h.win(mid));
    h.press();
    h.frame();
    h.release();
    h.frame();
    assert_eq!(h.st.selected_link, Some(Link { from: PortId::output(1, 0), to: PortId::input(2, 0) }));

    // Well away from the curve: nothing selected, and the background clears it.
    h.move_to(h.win(Vec2::new(mid.x, mid.y + 220.0)));
    h.press();
    h.frame();
    h.release();
    h.frame();
    assert_eq!(h.st.selected_link, None, "clicking empty canvas did not clear the link selection");
}
