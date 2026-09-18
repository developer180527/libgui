//! App state and panel UI. Panels don't know which window they live in; the
//! dock decides that.

use libgui::{Axis, Color, DockConfig, DockNode, DockState, Insets, Painter, Rect, ScrollOptions, Size, TabViewer, TextureId, Ui, Vec2};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Viewport,
    Outliner,
    Inspector,
    Console,
    Stats,
    DockTuning,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Tab::Viewport => "Scene",
            Tab::Outliner => "Outliner",
            Tab::Inspector => "Inspector",
            Tab::Console => "Console",
            Tab::Stats => "Stats",
            Tab::DockTuning => "Dock Tuning",
        }
    }
}

/// Default editor layout: outliner/inspector | scene/console | stats/tuning.
pub fn default_layout(dock: &mut DockState<Tab>) {
    let outliner = dock.leaf(vec![Tab::Outliner]);
    let inspector = dock.leaf(vec![Tab::Inspector]);
    let left = dock.split(Axis::Y, 0.45, outliner, inspector);
    let scene = dock.leaf(vec![Tab::Viewport]);
    let console = dock.leaf(vec![Tab::Console]);
    let center = dock.split(Axis::Y, 0.72, scene, console);
    let stats = dock.leaf(vec![Tab::Stats]);
    let tuning = dock.leaf(vec![Tab::DockTuning]);
    let right = dock.split(Axis::Y, 0.38, stats, tuning);
    let center_right = dock.split(Axis::X, 0.76, center, right);
    let root: DockNode<Tab> = dock.split(Axis::X, 0.2, left, center_right);
    dock.set_root(libgui::SurfaceId::MAIN, root);
}

/// A scene-ish object list, long enough to need scrolling (BIM-style names).
fn scene_objects() -> Vec<String> {
    let mut v: Vec<String> = ["Cube", "Ground grid", "Main camera", "Sun light", "Post volume"].map(String::from).to_vec();
    let kinds = ["Wall", "Beam", "Column", "Slab", "Door", "Window", "Duct", "Pipe"];
    v.extend((1..=55).map(|i| format!("{}_{i:03}", kinds[i % kinds.len()])));
    v
}

/// Application state the UI edits. Owned by the app, not the UI.
pub struct Demo {
    pub objects: Vec<String>,
    pub filter: String,
    pub console: Vec<String>,
    pub command: String,
    pub selected: usize,
    pub playing: bool,
    pub auto_rotate: bool,
    pub overlay: bool,
    pub spin_speed: f32,
    pub scale: f32,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub spin: f32,
    pub frame_ms: VecDeque<f32>,
    pub ui_instances: usize,
    pub ui_batches: usize,
    pub windows: usize,
    /// Viewport size in physical px, reported by whichever window shows it.
    pub viewport_px: (u32, u32),
    /// Edited by the Dock Tuning panel; the host copies it into the dock.
    pub dock_cfg: DockConfig,
    pub reset_layout: bool,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            objects: scene_objects(),
            filter: String::new(),
            console: vec!["libgui console ready".into(), "type 'help' for commands".into()],
            command: String::new(),
            selected: 0,
            playing: true,
            auto_rotate: true,
            overlay: true,
            spin_speed: 0.8,
            scale: 1.0,
            distance: 7.0,
            yaw: 0.6,
            pitch: 0.35,
            spin: 0.0,
            frame_ms: VecDeque::new(),
            ui_instances: 0,
            ui_batches: 0,
            windows: 1,
            viewport_px: (1, 1),
            dock_cfg: DockConfig::default(),
            reset_layout: false,
        }
    }
}

impl Demo {
    pub fn reset_view(&mut self) {
        self.distance = 7.0;
        self.yaw = 0.6;
        self.pitch = 0.35;
    }

    pub fn avg_ms(&self) -> f32 {
        if self.frame_ms.is_empty() {
            return 0.0;
        }
        self.frame_ms.iter().sum::<f32>() / self.frame_ms.len() as f32
    }

    pub fn log(&mut self, line: impl Into<String>) {
        self.console.push(line.into());
        if self.console.len() > 500 {
            self.console.remove(0);
        }
    }

    fn run_command(&mut self) {
        let cmd = std::mem::take(&mut self.command);
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        self.log(format!("> {cmd}"));
        let mut parts = cmd.split_whitespace();
        let arg = |p: Option<&str>| p.and_then(|v| v.parse::<f32>().ok());
        match parts.next().unwrap_or("") {
            "help" => self.log("help · clear · spin <v> · scale <v> · select <name> · add <name> · fill <n> · layout"),
            "clear" => self.console.clear(),
            "layout" => {
                self.reset_layout = true;
                self.log("layout reset");
            }
            "spin" => match arg(parts.next()) {
                Some(v) => {
                    self.spin_speed = v.clamp(0.0, 3.0);
                    self.log(format!("spin speed = {:.2}", self.spin_speed));
                }
                None => self.log("error: spin <number>"),
            },
            "scale" => match arg(parts.next()) {
                Some(v) => {
                    self.scale = v.clamp(0.3, 2.0);
                    self.log(format!("scale = {:.2}", self.scale));
                }
                None => self.log("error: scale <number>"),
            },
            "select" => {
                let q = parts.collect::<Vec<_>>().join(" ").to_lowercase();
                match self.objects.iter().position(|o| o.to_lowercase().contains(&q)) {
                    Some(i) => {
                        self.selected = i;
                        let name = self.objects[i].clone();
                        self.log(format!("selected {name}"));
                    }
                    None => self.log(format!("error: no object matches '{q}'")),
                }
            }
            "add" => {
                let name = parts.collect::<Vec<_>>().join(" ");
                let name = if name.is_empty() { format!("Object_{:03}", self.objects.len()) } else { name };
                self.objects.push(name.clone());
                self.selected = self.objects.len() - 1;
                self.log(format!("added {name}"));
            }
            "fill" => {
                let n = arg(parts.next()).unwrap_or(50.0) as usize;
                for i in 0..n {
                    self.log(format!("[trace] frame {i:04}  draw={}  ok", 3 + i % 5));
                }
            }
            other => self.log(format!("error: unknown command '{other}'")),
        }
    }
}

/// Draws panels for whichever window is being rendered.
pub struct Panels<'a> {
    pub d: &'a mut Demo,
    pub viewport_tex: TextureId,
    /// DPI scale of the window being rendered.
    pub scale: f32,
}

impl TabViewer for Panels<'_> {
    type Tab = Tab;

    fn title(&self, tab: &Tab) -> String {
        tab.title().to_string()
    }

    fn id(&self, tab: &Tab) -> u64 {
        *tab as u64
    }

    fn scroll(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Viewport | Tab::Console)
    }

    fn padding(&self, tab: &Tab) -> Insets {
        match tab {
            Tab::Viewport => Insets::all(0.0),
            _ => Insets::all(12.0),
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::Viewport => self.viewport(ui),
            Tab::Outliner => self.outliner(ui),
            Tab::Inspector => self.inspector(ui),
            Tab::Console => self.console(ui),
            Tab::Stats => self.stats(ui),
            Tab::DockTuning => dock_tuning(ui, &mut self.d.dock_cfg),
        }
    }
}

impl Panels<'_> {
    fn viewport(&mut self, ui: &mut Ui) {
        let d = &mut *self.d;
        let overlay = d.overlay;
        let stats = format!("{:.1} ms  ·  {:.0} fps", d.avg_ms(), 1000.0 / d.avg_ms().max(0.01));
        let title = d.objects[d.selected].clone();
        let resp = ui.viewport("main", self.viewport_tex, move |p: &mut Painter, r: Rect| {
            let t = p.theme;
            let small = t.font_size_small;
            if overlay {
                let w = p.measure(small, &stats).x + 20.0;
                let badge = Rect::new(r.x + 12.0, r.y + 12.0, w, 24.0);
                p.rect_bordered(badge, t.bg_panel.with_alpha(0.85), 12.0, 1.0, t.border);
                p.text_centered(badge, small, t.text, &stats);
            }
            let tw = p.measure(small, &title).x + 24.0;
            let tag = Rect::new(r.right() - tw - 12.0, r.y + 12.0, tw, 24.0);
            p.rect(tag, t.accent.with_alpha(0.2), 12.0);
            p.text_centered(tag, small, t.accent_hover, &title);
            p.text(Vec2::new(r.x + 14.0, r.bottom() - 26.0), small, t.text_faint, "Drag to orbit  ·  Scroll to zoom");
        });
        d.viewport_px = ((resp.rect.w * self.scale).round() as u32, (resp.rect.h * self.scale).round() as u32);
        d.yaw += resp.drag_delta.x * 0.008;
        d.pitch = (d.pitch + resp.drag_delta.y * 0.006).clamp(-0.2, 1.3);
        d.distance = (d.distance - resp.scroll.y * 0.02).clamp(3.0, 20.0);
    }

    fn outliner(&mut self, ui: &mut Ui) {
        let d = &mut *self.d;
        ui.text_input("search", &mut d.filter, "Search objects…");
        let filter = d.filter.to_lowercase();
        let mut picked = None;
        for (i, name) in d.objects.iter().enumerate() {
            if (filter.is_empty() || name.to_lowercase().contains(&filter)) && ui.selectable(name, d.selected == i).clicked {
                picked = Some(i);
            }
        }
        if let Some(i) = picked {
            d.selected = i;
            let name = d.objects[i].clone();
            d.log(format!("selected {name}"));
        }
    }

    fn inspector(&mut self, ui: &mut Ui) {
        let d = &mut *self.d;
        ui.section("Object");
        let sel = d.selected;
        if ui.text_input("name", &mut d.objects[sel], "Name").submitted {
            let name = d.objects[sel].clone();
            d.log(format!("renamed to {name}"));
        }
        ui.space(4.0);
        ui.section("Transform");
        ui.slider("Scale", &mut d.scale, 0.3, 2.0);
        ui.slider("Spin speed", &mut d.spin_speed, 0.0, 3.0);
        ui.toggle("Auto rotate", &mut d.auto_rotate);
        ui.space(4.0);
        ui.section("Camera");
        ui.slider("Distance", &mut d.distance, 3.0, 20.0);
        ui.slider("Pitch", &mut d.pitch, -0.2, 1.3);
        ui.toggle("Stats overlay", &mut d.overlay);
    }

    fn console(&mut self, ui: &mut Ui) {
        let d = &mut *self.d;
        let t = ui.theme.clone();
        let well = libgui::Frame { fill: t.bg_inset, border: t.border, border_width: 1.0, radius: t.radius, shadow: false, clip: true };
        ui.container(libgui::Layout::column().height(Size::Grow(1.0)), well, |ui| {
            let opts = ScrollOptions {
                gap: 3.0,
                padding: Insets { left: 10.0, top: 8.0, right: 14.0, bottom: 8.0 },
                stick_to_end: true,
                ..ScrollOptions::new(Size::Grow(1.0))
            };
            ui.scroll_area_with("console", opts, |ui| {
                for line in &d.console {
                    let color = if line.starts_with("> ") {
                        t.text
                    } else if line.starts_with("error") {
                        Color::hex(0xf87171)
                    } else {
                        t.text_muted
                    };
                    ui.text_with(line, t.font_size_small + 0.5, color);
                }
            });
        });
        let cmd = ui.text_input("command", &mut d.command, "Command…  (try: help)");
        if cmd.submitted {
            d.run_command();
            ui.set_focus(Some(cmd.response.id));
        }
    }

    fn stats(&mut self, ui: &mut Ui) {
        let d = &*self.d;
        let t = ui.theme.clone();
        ui.section("Frame");
        ui.text_with(&format!("{:.2} ms", d.avg_ms()), 22.0, t.text);
        let hist: Vec<f32> = d.frame_ms.iter().copied().collect();
        ui.plot("frame times", &hist, 33.3, 56.0);
        ui.label_muted(&format!("UI: {} instances · {} draw calls", d.ui_instances, d.ui_batches));
        ui.label_muted(&format!("Windows: {}", d.windows));
    }
}

/// Live-edit every docking feel parameter.
fn dock_tuning(ui: &mut Ui, c: &mut DockConfig) {
    ui.label_muted("Applies instantly while you drag.");
    if ui.button("Reset to defaults").clicked {
        *c = DockConfig::default();
    }
    ui.section("Drag");
    ui.slider("Drag threshold (px)", &mut c.drag_threshold, 1.0, 30.0);
    ui.slider("Tear-off distance (px)", &mut c.tear_off_distance, 4.0, 120.0);
    ui.toggle("Hide window over target", &mut c.hide_window_over_target);
    ui.section("Drop zones");
    ui.slider("Edge zone (fraction)", &mut c.edge_zone, 0.05, 0.49);
    ui.slider("Window edge (px)", &mut c.root_edge_px, 0.0, 80.0);
    ui.slider("Split share", &mut c.split_fraction, 0.2, 0.8);
    ui.slider("Window-edge share", &mut c.root_split_fraction, 0.1, 0.6);
    ui.section("Animation");
    ui.slider("Preview speed", &mut c.preview_speed, 4.0, 60.0);
    ui.slider("Reorder slide speed", &mut c.reorder_speed, 4.0, 60.0);
    ui.slider("Preview opacity", &mut c.preview_alpha, 0.05, 0.6);
    ui.section("Tabs & splitters");
    ui.slider("Tab height", &mut c.tab_height, 22.0, 44.0);
    ui.slider("Tab padding", &mut c.tab_padding, 6.0, 24.0);
    ui.slider("Tab radius", &mut c.tab_radius, 0.0, 12.0);
    ui.slider("Splitter gap", &mut c.splitter_size, 1.0, 10.0);
    ui.slider("Splitter grab pad", &mut c.splitter_hit_pad, 0.0, 10.0);
    ui.slider("Min pane size", &mut c.min_pane_size, 40.0, 300.0);
    ui.section("Current values (paste into DockConfig)");
    let dump = format!(
        "drag_threshold {:.1}, tear_off {:.1}, edge_zone {:.2}, root_edge {:.1}, split {:.2}, root_split {:.2}, preview {:.1}, reorder {:.1}",
        c.drag_threshold, c.tear_off_distance, c.edge_zone, c.root_edge_px, c.split_fraction, c.root_split_fraction, c.preview_speed, c.reorder_speed
    );
    let mut dump = dump;
    ui.text_input("dump", &mut dump, "");
}
