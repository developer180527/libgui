//! The per-frame builder: declare nodes, ports and links; get events back.

use crate::style::GraphStyle;
use crate::wire::{distance_to, wire_points};
use crate::{Drag, GraphEvent, GraphState, Link, NodeId, PortId, Side};
use libgui::{
    Color, Cursor, Frame, Id, Insets, Layer, Layout, LeafOptions, PointerButton, Rect, Size, Ui, Vec2,
};
use std::cell::RefCell;
use std::rc::Rc;

/// How one node looks and what ports it has.
#[derive(Clone, Copy, Debug)]
pub struct NodeConfig<'a> {
    pub title: &'a str,
    pub inputs: &'a [&'a str],
    pub outputs: &'a [&'a str],
    /// Overrides [`GraphStyle::node_width`].
    pub width: Option<f32>,
    /// Header stripe and selected border, for colouring by node category.
    pub accent: Option<Color>,
}

impl<'a> NodeConfig<'a> {
    pub fn new(title: &'a str) -> Self {
        Self { title, inputs: &[], outputs: &[], width: None, accent: None }
    }

    pub fn inputs(mut self, names: &'a [&'a str]) -> Self {
        self.inputs = names;
        self
    }

    pub fn outputs(mut self, names: &'a [&'a str]) -> Self {
        self.outputs = names;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn accent(mut self, c: Color) -> Self {
        self.accent = Some(c);
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NodeResponse {
    pub id: NodeId,
    /// Node rect in canvas units.
    pub rect: Rect,
    pub selected: bool,
    /// The header is under the pointer.
    pub hovered: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PortResponse {
    pub id: PortId,
    /// Where the link attaches, in canvas units.
    pub pos: Vec2,
    pub hovered: bool,
}

/// One port recorded this frame, so links and snapping can find it.
#[derive(Clone, Copy)]
struct PortRec {
    id: PortId,
    pos: Vec2,
}

/// A wire to draw, filled during the build and read by the paint closure —
/// which runs after the whole frame is built, so links can be drawn *under*
/// the nodes while still knowing where every port ended up.
#[derive(Clone)]
struct WireDraw {
    points: Vec<Vec2>,
    color: Color,
    width: f32,
}

pub struct GraphUi<'a> {
    ui: &'a mut Ui,
    style: GraphStyle,
    zoom: f32,
    pointer: Vec2,
    /// Pointer is over the canvas background (not over a node).
    ports: Vec<PortRec>,
    links: Vec<Link>,
    wires: Rc<RefCell<Vec<WireDraw>>>,
    events: Vec<GraphEvent>,
    selection: &'a mut std::collections::HashSet<NodeId>,
    selected_link: &'a mut Option<Link>,
    drag: &'a mut Option<Drag>,
    links_prev: &'a [Link],
    heights: &'a [(NodeId, f32)],
    new_heights: Vec<(NodeId, f32)>,
    scratch: Vec<Vec2>,
}

impl GraphUi<'_> {
    /// The libgui `Ui`, for drawing your own decorations in canvas space.
    pub fn ui(&mut self) -> &mut Ui {
        self.ui
    }

    pub fn style(&self) -> &GraphStyle {
        &self.style
    }

    pub fn is_selected(&self, node: NodeId) -> bool {
        self.selection.contains(&node)
    }

    /// Where a port sits, once its node has been declared this frame.
    pub fn port_pos(&self, port: PortId) -> Option<Vec2> {
        self.ports.iter().find(|p| p.id == port).map(|p| p.pos)
    }

    /// A node at `pos` (canvas units). `body` builds ordinary libgui widgets
    /// inside it, below the ports.
    pub fn node(&mut self, id: NodeId, pos: Vec2, cfg: &NodeConfig<'_>, body: impl FnOnce(&mut Ui)) -> NodeResponse {
        let s = self.style;
        let w = cfg.width.unwrap_or(s.node_width);
        let rows = cfg.inputs.len().max(cfg.outputs.len()) as f32;
        let ports_h = rows * s.port_row;
        // The body's height is measured; until it has been, reserve nothing.
        let body_h = self.heights.iter().find(|(n, _)| *n == id).map(|(_, h)| *h).unwrap_or(0.0);
        let rect = Rect::new(pos.x, pos.y, w, s.header_height + ports_h + body_h);
        let selected = self.selection.contains(&id);
        let accent = cfg.accent.unwrap_or(s.node_border_selected);

        let node_id = Id::new(("nodes_node", id));
        let head_id = node_id.with("header");

        // The header drags the node; the pointer is already in canvas units.
        let head = self.ui.interact_drag(head_id);
        if head.pressed {
            self.press_node(id);
        }
        if head.active {
            let d = head.drag_delta;
            if d.x != 0.0 || d.y != 0.0 {
                if let Some(Drag::Nodes { moved }) = self.drag.as_mut() {
                    *moved = true;
                }
                let targets: Vec<NodeId> = if self.selection.contains(&id) {
                    self.selection.iter().copied().collect()
                } else {
                    vec![id]
                };
                for n in targets {
                    self.events.push(GraphEvent::NodeMoved { node: n, delta: d });
                }
            }
        }
        if head.hovered {
            self.ui.cursor = Cursor::Grab;
        }

        let frame = Frame {
            fill: s.node_fill,
            border: if selected { accent } else { s.node_border },
            border_width: if selected { s.node_border_width * 2.0 } else { s.node_border_width },
            radius: s.node_radius,
            shadow: s.node_shadow,
            clip: true,
        };
        // A node being dragged goes above its neighbours.
        let z = if selected { Layer::Popup } else { Layer::Window };
        let title = cfg.title.to_string();
        let (header_fill, stripe_w, title_c, title_size) = (s.header_fill, s.header_stripe, s.title, s.title_size);
        let body_id = node_id.with("body");
        let ins: Vec<String> = cfg.inputs.iter().map(|s| s.to_string()).collect();
        let outs: Vec<String> = cfg.outputs.iter().map(|s| s.to_string()).collect();

        self.ui.container_at_in(node_id, z, rect, frame, |ui| {
            let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(s.header_height));
            ui.add_leaf(head_id, layout, Vec2::ZERO, true, move |p, r| {
                p.rect(r, header_fill, 0.0);
                if stripe_w > 0.0 {
                    p.rect(Rect::new(r.x, r.y, stripe_w, r.h), accent, 0.0);
                }
                p.text_left(r.shrink(stripe_w + 8.0, 0.0, 8.0, 0.0), title_size, title_c, &title);
            });
            // Port labels: the dots themselves are drawn on top, by `port`.
            let label_h = rows * s.port_row;
            if label_h > 0.0 {
                let lid = node_id.with("port_labels");
                let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(label_h));
                ui.add_leaf(lid, layout, Vec2::ZERO, false, move |p, r| {
                    for (i, name) in ins.iter().enumerate() {
                        let y = r.y + i as f32 * s.port_row;
                        let row = Rect::new(r.x + s.port_radius * 2.0 + 6.0, y, r.w, s.port_row);
                        p.text_left(row, s.port_label_size, s.port_label, name);
                    }
                    for (i, name) in outs.iter().enumerate() {
                        let y = r.y + i as f32 * s.port_row;
                        let row = Rect::new(r.x, y, r.w - s.port_radius * 2.0 - 6.0, s.port_row);
                        p.text_right(row, s.port_label_size, s.port_label, name);
                    }
                });
            }
            ui.container_id(
                body_id,
                Layout::column().width(Size::Grow(1.0)).height(Size::Fit).padding(Insets::all(8.0)).gap(6.0),
                Frame::none(),
                body,
            );
        });

        // Measure the body for next frame, so the node fits its contents.
        if let Some(r) = self.ui.rect_of(body_id) {
            self.new_heights.push((id, r.h));
        }

        // Record the ports, then draw them above the node.
        for (i, _) in cfg.inputs.iter().enumerate() {
            let y = rect.y + s.header_height + (i as f32 + 0.5) * s.port_row;
            self.add_port(PortId::input(id, i as u16), Vec2::new(rect.x, y));
        }
        for (i, _) in cfg.outputs.iter().enumerate() {
            let y = rect.y + s.header_height + (i as f32 + 0.5) * s.port_row;
            self.add_port(PortId::output(id, i as u16), Vec2::new(rect.right(), y));
        }

        NodeResponse { id, rect, selected, hovered: head.hovered }
    }

    /// Declare a link. Drawn under the nodes, and pickable.
    pub fn link(&mut self, from: PortId, to: PortId) {
        self.links.push(Link { from, to });
    }

    fn press_node(&mut self, id: NodeId) {
        let additive = self.ui.input().modifiers.shift || self.ui.input().modifiers.command;
        if additive {
            if !self.selection.insert(id) {
                self.selection.remove(&id);
            }
            self.events.push(GraphEvent::SelectionChanged);
        } else if !self.selection.contains(&id) {
            self.selection.clear();
            self.selection.insert(id);
            self.events.push(GraphEvent::SelectionChanged);
        }
        *self.selected_link = None;
        *self.drag = Some(Drag::Nodes { moved: false });
    }

    fn add_port(&mut self, id: PortId, pos: Vec2) {
        let s = self.style;
        self.ports.push(PortRec { id, pos });

        let wid = Id::new(("nodes_port", id.node, id.side as u8, id.index));
        let r = s.port_radius;
        let rect = Rect::new(pos.x - r, pos.y - r, r * 2.0, r * 2.0);
        let opts = LeafOptions { interactive: true, hit_pad: s.port_grab, hit_top: true };
        let resp = self.ui.interact_drag(wid);

        if resp.pressed {
            // Dragging an input that already has a link picks that link up,
            // rather than starting a second one: the standard "grab the end".
            let existing = (id.side == Side::In)
                .then(|| self.links_prev.iter().find(|l| l.to == id).copied())
                .flatten();
            match existing {
                Some(link) => {
                    self.events.push(GraphEvent::LinkRemoved { link });
                    *self.drag = Some(Drag::Link { from: link.from, detached: Some(link) });
                }
                None => *self.drag = Some(Drag::Link { from: id, detached: None }),
            }
        }
        if resp.hovered {
            self.ui.cursor = Cursor::Grab;
        }
        let hot = self.ui.animate_bool(wid, 0, resp.hovered || self.snap_target() == Some(id));
        let grow = 1.0 + 0.5 * hot;
        self.ui.add_leaf_at(wid, rect, opts, move |p, rr| {
            let c = rr.center();
            let rad = r * grow;
            let dot = Rect::new(c.x - rad, c.y - rad, rad * 2.0, rad * 2.0);
            p.rect_bordered(dot, s.port_fill.lerp(s.port_fill_hover, hot), rad, 1.0, s.port_border);
        });
    }

    /// The port a dragged link end would snap to, if any.
    fn snap_target(&self) -> Option<PortId> {
        let Some(Drag::Link { from, .. }) = *self.drag else { return None };
        let radius = self.style.snap_px / self.zoom.max(1e-6);
        let mut best: Option<(f32, PortId)> = None;
        for p in &self.ports {
            if p.id.side == from.side || p.id.node == from.node {
                continue;
            }
            let d = (p.pos.x - self.pointer.x).hypot(p.pos.y - self.pointer.y);
            if d <= radius && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, p.id));
            }
        }
        best.map(|(_, id)| id)
    }
}

/// Run a node graph for one frame. Returns the edits the user made, to apply
/// to your own graph.
pub fn graph<R>(
    ui: &mut Ui,
    key: &str,
    st: &mut GraphState,
    style: &GraphStyle,
    body: impl FnOnce(&mut GraphUi) -> R,
) -> (Vec<GraphEvent>, R) {
    // Disjoint borrows: the canvas takes the view, the builder takes the rest.
    let GraphState { view, selection, selected_link, drag, links_prev, heights } = st;
    let s = *style;
    let mut out_events = Vec::new();
    let mut out_links = Vec::new();
    let mut out_heights = Vec::new();
    let released = !ui.button_down(PointerButton::Primary);

    let (bg, (r, wire_hit)) = ui.canvas(key, view, |ui, cview| {
        let wires: Rc<RefCell<Vec<WireDraw>>> = Rc::new(RefCell::new(Vec::new()));
        // Added first so it paints under the nodes; its closure runs after the
        // whole frame is built, so by then every port position is known.
        let layer_id = Id::new(("nodes_wires", key));
        let buf = wires.clone();
        ui.add_leaf_at(layer_id, cview.visible, LeafOptions::default(), move |p, _| {
            for w in buf.borrow().iter() {
                p.polyline(&w.points, w.width, w.color);
            }
        });

        let mut g = GraphUi {
            ui,
            style: s,
            zoom: cview.zoom,
            pointer: Vec2::ZERO,
            ports: Vec::new(),
            links: Vec::new(),
            wires: wires.clone(),
            events: Vec::new(),
            selection,
            selected_link,
            drag,
            links_prev,
            heights,
            new_heights: Vec::new(),
            scratch: Vec::new(),
        };
        g.pointer = g.ui.input().mouse_pos;
        // Inside the canvas the raw pointer is still in window space; map it.
        g.pointer = g.ui.xform().inv_point(g.pointer);

        let r = body(&mut g);

        // Returns whether a wire took the press: wires are hit-tested here, not
        // by libgui, so the canvas background cannot know on its own.
        let wire_hit = finish(&mut g, cview.zoom, released);

        out_events = std::mem::take(&mut g.events);
        out_links = std::mem::take(&mut g.links);
        out_heights = std::mem::take(&mut g.new_heights);
        (r, wire_hit)
    });

    st.links_prev = out_links;
    st.heights = out_heights;
    if bg.pressed && !wire_hit {
        st.selection.clear();
        st.selected_link = None;
        out_events.push(GraphEvent::SelectionChanged);
        out_events.push(GraphEvent::BackgroundClicked { pos: bg.mouse_pos });
    }
    (out_events, r)
}

/// Everything that needs every port and link: wire geometry, picking, and
/// finishing a drag.
fn finish(g: &mut GraphUi, zoom: f32, released: bool) -> bool {
    let s = g.style;
    let width = s.link_width.max(s.link_min_px / zoom.max(1e-6));
    let grab = s.link_grab_px / zoom.max(1e-6);

    // Which link is under the pointer? Nearest wins, so crossing wires behave.
    // A link whose ports were not declared this frame draws no wire, so the
    // hovered wire is remembered by its index in `wires`, which is not the
    // link's index in `links`.
    let mut hover: Option<(f32, Link, usize)> = None;
    let mut scratch = std::mem::take(&mut g.scratch);
    let links = g.links.clone();
    for l in &links {
        let (Some(a), Some(b)) = (g.port_pos(l.from), g.port_pos(l.to)) else { continue };
        wire_points(a, b, s.routing, zoom, &mut scratch);
        let d = distance_to(&scratch, g.pointer);
        let selected = *g.selected_link == Some(*l);
        let color = if selected { s.link_selected } else { s.link };
        let mut w = g.wires.borrow_mut();
        if d <= grab && hover.is_none_or(|(bd, ..)| d < bd) {
            hover = Some((d, *l, w.len()));
        }
        w.push(WireDraw { points: scratch.clone(), color, width });
    }
    // Recolour the hovered one, now that we know which it is.
    if let Some((_, l, i)) = hover {
        let mut w = g.wires.borrow_mut();
        if *g.selected_link != Some(l) {
            w[i].color = s.link_hover;
        }
        w[i].width = width * 1.3;
        drop(w);
        g.ui.cursor = Cursor::Pointer;
    }

    // Clicking a wire selects it; clicking empty canvas is handled by the
    // background response, which does not fire when a wire is under the pointer.
    let pressed = g.ui.button_pressed(PointerButton::Primary);
    let mut wire_hit = false;
    if pressed && g.drag.is_none() {
        if let Some((_, l, _)) = hover {
            *g.selected_link = Some(l);
            g.selection.clear();
            g.events.push(GraphEvent::SelectionChanged);
            wire_hit = true;
        }
    }

    // The link being dragged: its loose end eases into a port it has caught,
    // which is the snap. `keep_id` because this id is not a widget, so its
    // animation would otherwise be collected at the end of the frame.
    if let Some(Drag::Link { from, detached }) = *g.drag {
        let anchor = g.port_pos(from).unwrap_or(g.pointer);
        let target = g.snap_target();
        let dest = target.and_then(|t| g.port_pos(t)).unwrap_or(g.pointer);
        let aid = Id::new(("nodes_drag_end", key_of(from)));
        g.ui.keep_id(aid);
        let end = Vec2::new(
            g.ui.animate_with_speed(aid, 0, dest.x, s.snap_speed),
            g.ui.animate_with_speed(aid, 1, dest.y, s.snap_speed),
        );
        let (a, b) = if from.side == Side::Out { (anchor, end) } else { (end, anchor) };
        wire_points(a, b, s.routing, zoom, &mut scratch);
        let color = if target.is_some() { s.link_selected } else { s.link_invalid };
        g.wires.borrow_mut().push(WireDraw { points: scratch.clone(), color, width: width * 1.2 });
        g.ui.cursor = Cursor::Grabbing;

        if released {
            if let Some(t) = target {
                let link = if from.side == Side::Out {
                    Link { from, to: t }
                } else {
                    Link { from: t, to: from }
                };
                g.events.push(GraphEvent::LinkCreated { link });
            } else if let Some(old) = detached {
                // Dropped on nothing: the pick-up stands, so the link is gone.
                let _ = old;
            }
            *g.drag = None;
        }
    } else if released {
        *g.drag = None;
    }
    g.scratch = scratch;
    wire_hit
}

fn key_of(p: PortId) -> (u64, u8, u16) {
    (p.node, p.side as u8, p.index)
}
