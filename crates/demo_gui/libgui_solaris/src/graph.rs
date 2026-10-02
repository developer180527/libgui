//! The network editor: the same `libgui_nodes` the first demo uses, with this
//! scene's nodes in it.

use crate::widgets::*;
use crate::Editor;
use libgui::*;
use libgui_nodes::{GraphStyle, Link, NodeConfig, NodeId, PortId};

pub struct Node {
    pub id: NodeId,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub pos: Vec2,
    pub tint: Color,
    pub ins: &'static [&'static str],
    pub outs: &'static [&'static str],
}

pub fn nodes() -> Vec<Node> {
    let n = |id, title, subtitle, x, y, tint, ins, outs| Node {
        id,
        title,
        subtitle,
        pos: Vec2::new(x, y),
        tint,
        ins,
        outs,
    };
    const IO: &[&str] = &["in"];
    const OUT: &[&str] = &["out"];
    vec![
        n(0, "materiallibrary1", "/Lightbulb/material/black_metal… (8)", 150.0, 18.0, Color::hex(0x8a9a4a), IO, OUT),
        n(1, "rendergeometrysettings2", "/Lightbulb/_24_lightbulb/Bulb… (2)", 150.0, 112.0, Color::hex(0x4a7aa8), IO, OUT),
        n(2, "light1_bulb", "/lights/light1_bulb", 372.0, 52.0, Color::hex(0xc8a63c), IO, OUT),
        n(3, "light1_rim", "/lights/light1_rim", 560.0, 52.0, Color::hex(0xc0489a), IO, OUT),
        n(4, "merge6", "2 Layers", 396.0, 158.0, Color::hex(0x6a6a7a), &["a", "b"], OUT),
        n(5, "payload4", "/Content/Butterfly_01", 8.0, 222.0, Color::hex(0xc0489a), &[], OUT),
        n(6, "rendergeometrysettings1", "3 Layers", 224.0, 218.0, Color::hex(0x4a7aa8), IO, OUT),
        n(7, "sceneimport1", "", 452.0, 222.0, Color::hex(0x6a6a7a), IO, OUT),
        n(8, "merge2", "4 Layers", 150.0, 294.0, Color::hex(0x6a6a7a), &["a", "b"], OUT),
        n(9, "usd_rop8", "ground_geo.usd", 452.0, 294.0, Color::hex(0xc04a5a), IO, &[]),
    ]
}

pub fn links() -> Vec<Link> {
    let l = |a: NodeId, b: NodeId, bi: u16| Link { from: PortId::output(a, 0), to: PortId::input(b, bi) };
    vec![l(0, 1, 0), l(2, 4, 0), l(3, 4, 1), l(4, 6, 0), l(6, 8, 0), l(5, 8, 1), l(7, 9, 0), l(1, 8, 0)]
}

pub fn network(ui: &mut Ui, app: &mut Editor) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
    ui.container_id(Id::new("netpanel"), col, Frame::none(), |ui| {
        breadcrumb(ui, "net", "stage");
        net_menu(ui);
        let mut style = GraphStyle::from_theme(&t);
        style.grid = Color::hex(0x202020);
        style.node_width = 152.0;
        style.node_radius = 3.0;
        style.node_shadow = false;
        style.header_height = 17.0;
        style.title_size = 10.5;
        style.port_label_size = 9.0;
        style.port_radius = 3.0;
        let nodes = &app.nodes;
        let links = &app.links;
        let (_events, ()) = libgui_nodes::graph(ui, "net", &mut app.graph, &style, |g| {
            for n in nodes {
                let cfg = NodeConfig::new(n.title).inputs(n.ins).outputs(n.outs).accent(n.tint);
                let sub = n.subtitle;
                g.node(n.id, n.pos, &cfg, |ui| {
                    if !sub.is_empty() {
                        ui.label_muted(sub);
                    }
                });
            }
            for l in links {
                g.link(l.from, l.to);
            }
        });
    });
}

fn net_menu(ui: &mut Ui) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(19.0))
        .padding(Insets::xy(4.0, 0.0))
        .gap(2.0)
        .align(Align::Start, Align::Center);
    ui.container_id(Id::new("netmenu"), row, Frame { fill: t.palette.bg_panel, ..Frame::none() }, |ui| {
        for m in ["Add", "Edit", "Go", "View", "Tools", "Layout", "Labs", "Help"] {
            ui.menu_button(m, |ui| {
                let _ = ui.menu_item("…");
            });
        }
        ui.flex();
        tool_row(ui, "nt", 8, 15.0);
    });
}
