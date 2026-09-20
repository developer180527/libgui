//! The viewport, the parameter editor, and the two tables under the viewport.

use crate::widgets::*;
use crate::App;
use libgui::*;

// ---- viewport --------------------------------------------------------------

pub fn viewport(ui: &mut Ui, app: &mut App) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
    ui.container_id(Id::new("viewpanel"), col, panel_frame(&t), |ui| {
        tabs(ui, "view", &mut app.view_tab, &["Scene View", "Animation Editor", "Geometry Spreadsheet"]);
        view_toolbar(ui);
        // The render, and the toolbars that sit over it.
        let body = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container_id(Id::new("viewbody"), body, Frame { fill: Color::hex(0x0a0a0a), clip: true, ..Frame::none() }, |ui| {
            side_rail(ui, "left", 11);
            let stage = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
            ui.container_id(Id::new("stage"), stage, Frame::none(), |ui| {
                render(ui, app);
            });
            side_rail(ui, "right", 9);
        });
    });
}

fn view_toolbar(ui: &mut Ui) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(21.0))
        .padding(Insets::xy(4.0, 0.0))
        .gap(3.0)
        .align(Align::Start, Align::Center);
    ui.container_id(Id::new("viewtools"), row, Frame { fill: t.palette.bg_panel, ..Frame::none() }, |ui| {
        swatch(ui, "viewicon", t.palette.text_faint);
        ui.label("View");
        ui.flex();
        tool_row(ui, "vt", 7, 15.0);
        ui.space(4.0);
        tool_icon(ui, "vtgear", 3, 15.0);
    });
}

fn side_rail(ui: &mut Ui, key: &str, n: usize) {
    let t = ui.theme.clone();
    let col = Layout::column()
        .width(Size::Fixed(20.0))
        .height(Size::Grow(1.0))
        .padding(Insets::xy(2.0, 3.0))
        .gap(3.0);
    let __id = ui.make_id(("rail", key));
        ui.container_id(__id, col, Frame { fill: t.palette.bg_panel, ..Frame::none() }, |ui| {
        for i in 0..n {
            tool_icon(ui, ("rail", key, i), i as u8, 16.0);
        }
    });
}

/// Stand-in for the render itself. Not the photograph — a path tracer's
/// half-converged output, which is a cloud of bright samples over black, and
/// which is a fair thing to ask the renderer to draw a few thousand of.
fn render_preview(ui: &mut Ui) {
    let id = ui.make_id("preview");
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, |p, r| {
        // A deterministic scatter, so the picture is the same every run and a
        // golden image of it means something.
        let mut seed = 0x9e3779b9u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / 16_777_216.0
        };
        let c = Vec2::new(r.x + r.w * 0.47, r.y + r.h * 0.5);
        for _ in 0..2600 {
            // Two lobes — a wing above, a bulb below — so the scatter reads as
            // a subject rather than as noise.
            let (u, v, w) = (rnd(), rnd(), rnd());
            let wing = w < 0.55;
            let (rx, ry, ox, oy) = if wing { (0.22, 0.26, 0.13, -0.24) } else { (0.26, 0.15, -0.06, 0.20) };
            let a = u * std::f32::consts::TAU;
            let rad = v.sqrt();
            let x = c.x + (a.cos() * rad * rx + ox) * r.w;
            let y = c.y + (a.sin() * rad * ry + oy) * r.h;
            if !r.contains(Vec2::new(x, y)) {
                continue;
            }
            let heat = 1.0 - rad;
            let col = if wing {
                Color::hex(0xd8622a).lerp(Color::hex(0xe8c27a), v)
            } else {
                Color::hex(0x6a6a72).lerp(Color::WHITE, heat * 0.8)
            };
            let s = 0.7 + heat * 1.3;
            p.rect(Rect::new(x, y, s, s), col.with_alpha(0.25 + heat * 0.6), s * 0.5);
        }
    });
}

/// Stand-in for the render: the HUD a viewport wears, over the dark.
fn render(ui: &mut Ui, app: &mut App) {
    let t = ui.theme.clone();
    let stage = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
    ui.container_id(Id::new("preview_holder"), stage, Frame { clip: true, ..Frame::none() }, |ui| {
        render_preview(ui);
    });
    let rect = ui.rect_of(Id::new("preview_holder")).unwrap_or_default();
    let stack = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(6.0)).gap(4.0);
    ui.container_at(Id::new("render"), rect, Frame::none(), |ui| {
        let _ = stack;
        let top = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(18.0)).gap(4.0).align(Align::End, Align::Center);
        ui.container_id(Id::new("hudtop"), top, Frame::none(), |ui| {
            ui.flex();
            tool_icon(ui, "hudgear", 3, 15.0);
            hud_pick(ui, "renderer", "RenderMan XPU  Persp", &mut app.renderer, 138.0);
            hud_pick(ui, "cam", "No cam", &mut app.camera, 62.0);
        });
        ui.flex();
        let help = "Left mouse tumbles. Middle pans. Right dollies. Ctrl+Left box-zooms. Ctrl+Right zooms. \
                    Spacebar-Ctrl-Left tilts. Hold L for alternate tumble, dolly, and zoom.    M or Alt+M for First Person Navigation.";
        let text = ui.frame_text(help);
        let size = t.metrics.font_size_small;
        let fg = t.palette.text_faint;
        let edition = ui.frame_text("Indie Edition");
        let id = ui.make_id("hint");
        ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(14.0)), Vec2::ZERO, false, move |p, r| {
            p.text_centered(r, size, fg, text);
            p.text_right(r, size, fg, edition);
        });
    });
}

fn hud_pick(ui: &mut Ui, key: &str, label: &str, _sel: &mut usize, w: f32) {
    let t = ui.theme.clone();
    let f = Frame { fill: Color::hex(0x242424), border: t.palette.border_strong, border_width: 1.0, radius: 8.0, ..Frame::none() };
    let row = Layout::row()
        .width(Size::Fixed(w))
        .height(Size::Fixed(16.0))
        .padding(Insets::xy(7.0, 0.0))
        .align(Align::Start, Align::Center);
    let __id = ui.make_id(("hud", key));
        ui.container_id(__id, row, f, |ui| {
        ui.label(label);
        ui.flex();
        let c = t.palette.text_faint;
        let id = ui.make_id(("hudarrow", key));
        ui.add_leaf(id, Layout::leaf(Size::Fixed(8.0), Size::Fixed(8.0)), Vec2::ZERO, false, move |p, r| {
            p.chevron(r, 7.0, Chevron::Down, c);
        });
    });
}

// ---- parameters ------------------------------------------------------------

const ATTRS: [&str; 10] = [
    "Ri Matte",
    "Holdout",
    "Reverse Orientation",
    "Sides",
    "Identifier LPE Group",
    "Max Diffuse Depth",
    "Max Specular Depth",
    "Relative Pixel Variance",
    "Intersect Priority",
    "",
];

pub fn parameters(ui: &mut Ui, app: &mut App) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(0.52));
    ui.container_id(Id::new("parampanel"), col, panel_frame(&t), |ui| {
        tabs(
            ui,
            "param",
            &mut app.param_tab,
            &["rendergeometrysettings2", "Context Options Editor", "Performance Monitor", "Render Scheduler"],
        );
        breadcrumb(ui, "param", "stage");
        let body = Layout::column()
            .width(Size::Grow(1.0))
            .height(Size::Grow(1.0))
            .padding(Insets::all(5.0))
            .gap(3.0);
        ui.container_id(Id::new("parambody"), body, Frame::none(), |ui| {
            title_row(ui);
            field_row(ui, "asset", "Asset Name and Path", &["rendergeometrysettings", "/opt/hfs19.0.589/houdini/otls/OPlibLop.hda"]);
            field_row(ui, "prims", "Primitives", &["/Lightbulb/_24_lightbulb/Bulb  /Lightbulb/_24_lightbulb/light_glass"]);
            field_row(ui, "init", "Initialize Parameters", &["Initialize Parameters"]);
            ui.space(2.0);
            tabs(ui, "paramsub", &mut app.param_sub, &["RenderMan RIS 24.0", "Karma (Beta)"]);
            tabs(ui, "attrs", &mut app.attr_tab, &["Instance Attributes", "Master Attributes"]);
            ui.space(2.0);
            attribute_rows(ui, app);
        });
    });
}

fn title_row(ui: &mut Ui) {
    let t = ui.theme.clone();
    let row = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(18.0)).gap(5.0).align(Align::Start, Align::Center);
    ui.container_id(Id::new("paramtitle"), row, Frame::none(), |ui| {
        swatch(ui, "paramicon", t.palette.accent);
        ui.label("Render Geometry Settings");
        ui.label_muted("rendergeometrysettings2");
        ui.flex();
        tool_row(ui, "paramtools", 4, 14.0);
    });
}

fn field_row(ui: &mut Ui, key: &str, label: &str, fields: &[&str]) {
    let t = ui.theme.clone();
    let row = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(17.0)).gap(4.0).align(Align::Start, Align::Center);
    let __id = ui.make_id(("field", key));
        ui.container_id(__id, row, Frame::none(), |ui| {
        let w = 118.0;
        let text = ui.frame_text(label);
        let size = t.metrics.font_size;
        let fg = t.palette.text_muted;
        let id = ui.make_id(("fieldlabel", key));
        ui.add_leaf(id, Layout::leaf(Size::Fixed(w), Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| {
            p.text_right(r, size, fg, text);
        });
        for (i, f) in fields.iter().enumerate() {
            let frame =
                Frame { fill: t.palette.bg_inset, border: t.palette.border_strong, border_width: 1.0, radius: 2.0, ..Frame::none() };
            let cell = Layout::row()
                .width(Size::Grow(1.0))
                .height(Size::Fixed(15.0))
                .padding(Insets::xy(5.0, 0.0))
                .align(Align::Start, Align::Center);
            let __id = ui.make_id(("fieldbox", key, i));
        ui.container_id(__id, cell, frame, |ui| {
                ui.label(f);
                ui.flex();
                let c = t.palette.text_faint;
                let aid = ui.make_id(("fieldarrow", key, i));
                ui.add_leaf(aid, Layout::leaf(Size::Fixed(8.0), Size::Fixed(8.0)), Vec2::ZERO, false, move |p, r| {
                    p.chevron(r, 7.0, Chevron::Down, c);
                });
            });
        }
    });
}

/// The block that gives this panel its shape: a column of identical pickers on
/// the left, and the attribute each one drives on the right.
fn attribute_rows(ui: &mut Ui, app: &mut App) {
    let grid = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0)).gap(14.0);
    ui.container_id(Id::new("attrgrid"), grid, Frame::none(), |ui| {
        let left = Layout::column().width(Size::Fixed(92.0)).height(Size::Fit).gap(3.0);
        ui.container_id(Id::new("attrleft"), left, Frame::none(), |ui| {
            for i in 0..ATTRS.len() {
                let label = if i + 1 == ATTRS.len() { "Set or Create" } else { "Do Nothing" };
                let mut sel = app.attrs[i];
                ui.with_key(("attr", i), |ui| {
                    ui.combo(label, &mut sel, &[label]);
                });
                app.attrs[i] = sel;
            }
        });
        let right = Layout::column().width(Size::Grow(1.0)).height(Size::Fit).gap(3.0);
        ui.container_id(Id::new("attrright"), right, Frame::none(), |ui| {
            for (i, name) in ATTRS.iter().enumerate() {
                attribute_row(ui, i, name, app);
            }
        });
    });
}

/// Labels are right-aligned into a column of their own, the way a parameter
/// editor lines a hundred of them up.
const LABEL_W: f32 = 148.0;

fn attribute_row(ui: &mut Ui, i: usize, name: &str, app: &mut App) {
    let t = ui.theme.clone();
    let row = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(18.0)).gap(5.0).align(Align::Start, Align::Center);
    let __id = ui.make_id(("attrrow", i));
        ui.container_id(__id, row, Frame::none(), |ui| {
        match i {
            3 => {
                label_right(ui, ("sides", i), "Sides", LABEL_W);
                let mut sel = app.sides;
                ui.combo("Double Sided", &mut sel, &["Double Sided", "Single Sided"]);
                app.sides = sel;
                ui.flex();
            }
            0..=2 => {
                label_right(ui, ("attrname", i), name, LABEL_W);
                ui.flex();
            }
            4 => {
                label_right(ui, ("attrname", i), name, LABEL_W);
                inset(ui, ("lpe", i), Size::Grow(1.0));
            }
            5..=8 => {
                label_right(ui, ("attrname", i), name, LABEL_W);
                inset(ui, ("num", i), Size::Fixed(34.0));
                let track = t.palette.bg_inset;
                let id = ui.make_id(("slider", i));
                ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(12.0)), Vec2::ZERO, false, move |p, r| {
                    p.rect(Rect::new(r.x, r.center().y - 1.0, r.w, 2.0), track, 1.0);
                });
            }
            _ => {
                // The three visibility toggles share the last row's column.
                ui.space(LABEL_W + 5.0);
                let col = Layout::column().width(Size::Grow(1.0)).height(Size::Fit).gap(3.0);
                ui.container_id(Id::new("vis"), col, Frame::none(), |ui| {
                    ui.checkbox("Camera Visibility", &mut app.camera_vis);
                    ui.checkbox("Indirect Visibility", &mut app.indirect_vis);
                    ui.checkbox("Transmission Visibility", &mut app.transmission_vis);
                });
            }
        }
    });
}

fn label_right(ui: &mut Ui, key: impl std::hash::Hash, label: &str, w: f32) {
    let t = ui.theme.clone();
    let text = ui.frame_text(label);
    let size = t.metrics.font_size;
    let fg = t.palette.text_muted;
    let id = ui.make_id(key);
    ui.add_leaf(id, Layout::leaf(Size::Fixed(w), Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| {
        p.text_right(r, size, fg, text);
    });
}

fn inset(ui: &mut Ui, key: impl std::hash::Hash, w: Size) {
    let t = ui.theme.clone();
    let f = Frame { fill: t.palette.bg_inset, border: t.palette.border_strong, border_width: 1.0, radius: 2.0, ..Frame::none() };
    let cell = Layout::row().width(w).height(Size::Fixed(14.0));
    let __id = ui.make_id(key);
    ui.container_id(__id, cell, f, |ui| {
        ui.space(2.0);
    });
}

// ---- the tables under the viewport -----------------------------------------

/// One row of the scene graph tree.
pub struct Row {
    pub depth: usize,
    pub name: &'static str,
    pub kind: &'static str,
    pub descendants: &'static str,
    pub variants: &'static str,
    pub prim_kind: &'static str,
    pub branch: Branch,
}

pub fn tree() -> Vec<Row> {
    let r = |depth, name, kind, descendants, variants, prim_kind, branch| Row {
        depth,
        name,
        kind,
        descendants,
        variants,
        prim_kind,
        branch,
    };
    vec![
        r(0, "Favorites", "", "", "", "", Branch::Collapsed),
        r(0, "/", "", "37", "", "", Branch::Expanded),
        r(1, "Lightbulb", "Xform", "37", "", "", Branch::Expanded),
        r(2, "_24_lightbulb", "Xform", "9", "", "compon", Branch::Expanded),
        r(3, "Bulb", "Mesh", "1", "", "", Branch::Leaf),
        r(3, "black_metal", "Mesh", "1", "", "", Branch::Leaf),
        r(3, "bottom_glass", "Mesh", "1", "", "", Branch::Leaf),
        r(3, "contact", "Mesh", "1", "", "", Branch::Leaf),
        r(3, "draht", "Mesh", "1", "", "", Branch::Leaf),
        r(3, "light_glass", "Mesh", "1", "", "", Branch::Leaf),
    ]
}

pub fn tree_columns() -> TableState {
    TableState::new([
        Column::new("Scene Graph Path").width(200.0).grow(1.0),
        Column::new("Primitive Type").width(84.0),
        Column::new("Descendants").width(74.0),
        Column::new("Variants").width(58.0),
        Column::new("Kind").width(44.0),
        Column::new("Draw Mode").width(66.0),
        Column::new("P L A V S").width(62.0),
    ])
    .frozen(1)
}

pub fn detail_columns() -> TableState {
    TableState::new([Column::new("Name").width(80.0), Column::new("Value").width(120.0).grow(1.0)])
}

pub fn scene_graph_tree(ui: &mut Ui, app: &mut App) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(0.52)).height(Size::Grow(1.0));
    ui.container_id(Id::new("treepanel"), col, panel_frame(&t), |ui| {
        tabs(ui, "tree", &mut app.tree_tab, &["Scene Graph Tree"]);
        breadcrumb(ui, "tree", "stage");
        tree_toolbar(ui);
        let rows = app.tree.len();
        let opts = TableOptions { selected: app.selected_prim, striped: false, ..default_table(ui) };
        let tree = &app.tree;
        let r = ui.table_with("tree", &mut app.cols, rows, opts, |ui, row, col| {
            let it = &tree[row];
            match col {
                0 => {
                    ui.space(it.depth as f32 * 11.0);
                    let _ = ui.tree_row(("t", row), 0, it.branch, it.name, false);
                }
                1 => ui.label_muted(it.kind),
                2 => ui.label_muted(it.descendants),
                3 => ui.label_muted(it.variants),
                4 => ui.label_muted(it.prim_kind),
                5 => {}
                _ => ui.label_muted("· · · · ·"),
            }
        });
        if let Some(i) = r.clicked_row {
            app.selected_prim = Some(i);
        }
    });
}

fn tree_toolbar(ui: &mut Ui) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(19.0))
        .padding(Insets::xy(4.0, 0.0))
        .gap(3.0)
        .align(Align::Start, Align::Center);
    ui.container_id(Id::new("treetools"), row, Frame { fill: t.palette.bg_panel, ..Frame::none() }, |ui| {
        let f = Frame { fill: t.palette.bg_inset, border: t.palette.border_strong, border_width: 1.0, radius: 2.0, ..Frame::none() };
        let cell = Layout::row()
            .width(Size::Fixed(126.0))
            .height(Size::Fixed(15.0))
            .padding(Insets::xy(5.0, 0.0))
            .align(Align::Start, Align::Center);
        ui.container_id(Id::new("treemode"), cell, f, |ui| {
            ui.label("Composed scene graph");
        });
        ui.flex();
        tool_row(ui, "tt", 6, 15.0);
    });
}

pub fn scene_graph_details(ui: &mut Ui, app: &mut App) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(0.48)).height(Size::Grow(1.0));
    ui.container_id(Id::new("detailpanel"), col, panel_frame(&t), |ui| {
        tabs(ui, "detail", &mut app.detail_tab, &["Scene Graph Details", "Scene Graph Layers", "Layout Asset Gallery"]);
        breadcrumb(ui, "detail", "stage");
        detail_toolbar(ui);
        let opts = TableOptions { selected: Some(0), striped: false, ..default_table(ui) };
        let _ = ui.table_with("detail", &mut app.detail_cols, 1, opts, |_, _, _| {});
    });
}

fn detail_toolbar(ui: &mut Ui) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(19.0))
        .padding(Insets::xy(4.0, 0.0))
        .gap(3.0)
        .align(Align::Start, Align::Center);
    ui.container_id(Id::new("detailtools"), row, Frame { fill: t.palette.bg_panel, ..Frame::none() }, |ui| {
        tool_row(ui, "dt", 4, 15.0);
        ui.flex();
        tool_icon(ui, "dtfilter", 5, 15.0);
    });
}

pub fn default_table(ui: &Ui) -> TableOptions {
    let s = ui.theme.table;
    TableOptions {
        row_height: s.row_height,
        header_height: s.header_height,
        height: Size::Grow(1.0),
        selected: None,
        striped: false,
        grid_lines: true,
    }
}
