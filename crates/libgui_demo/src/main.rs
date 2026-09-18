//! Editor-style demo: retained-feeling panels built with the immediate API,
//! around an engine viewport rendered offscreen by "your" renderer.

mod scene;

use libgui::{Backend, Color, Cursor, Event, Frame, Input, Insets, Key, Layout, Modifiers, Painter, Rect, ScrollOptions, Size, TextureId, Ui, Vec2};
use scene::{Scene, SceneParams};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{CursorIcon, Window, WindowId};

/// A scene-ish object list, long enough to need scrolling (BIM-style names).
fn scene_objects() -> Vec<String> {
    let mut v: Vec<String> = ["Cube", "Ground grid", "Main camera", "Sun light", "Post volume"].map(String::from).to_vec();
    let kinds = ["Wall", "Beam", "Column", "Slab", "Door", "Window", "Duct", "Pipe"];
    v.extend((1..=55).map(|i| format!("{}_{i:03}", kinds[i % kinds.len()])));
    v
}

/// Application state the UI edits. Owned by the app, not the UI.
struct Demo {
    objects: Vec<String>,
    filter: String,
    console: Vec<String>,
    command: String,
    selected: usize,
    playing: bool,
    auto_rotate: bool,
    overlay: bool,
    spin_speed: f32,
    scale: f32,
    distance: f32,
    yaw: f32,
    pitch: f32,
    spin: f32,
    frame_ms: VecDeque<f32>,
    ui_instances: usize,
    ui_batches: usize,
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
        }
    }
}

impl Demo {
    fn reset_view(&mut self) {
        self.distance = 7.0;
        self.yaw = 0.6;
        self.pitch = 0.35;
    }

    fn log(&mut self, line: impl Into<String>) {
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
            "help" => {
                self.log("help · clear · spin <v> · scale <v> · select <name> · add <name> · fill <n>");
            }
            "clear" => self.console.clear(),
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

    fn avg_ms(&self) -> f32 {
        if self.frame_ms.is_empty() {
            return 0.0;
        }
        self.frame_ms.iter().sum::<f32>() / self.frame_ms.len() as f32
    }
}

/// The whole editor UI, rebuilt every frame. Returns the viewport's rect.
fn build_ui(ui: &mut Ui, d: &mut Demo, viewport: TextureId) -> Rect {
    let t = ui.theme.clone();
    let mut viewport_rect = Rect::default();

    // ── Top bar ────────────────────────────────────────────────────────────
    let bar = Layout::row().height(Size::Fixed(48.0)).padding(Insets::xy(14.0, 0.0)).gap(8.0);
    ui.container(bar, Frame { clip: false, ..Frame::panel(&t) }, |ui| {
        let id = ui.make_id("logo");
        ui.add_leaf(id, Layout::leaf(Size::Fixed(18.0), Size::Fixed(18.0)), Vec2::ZERO, false, |p, r| {
            let t = p.theme;
            p.shadow(r, 5.0, 8.0, t.accent.with_alpha(0.5));
            p.rect(r, t.accent, 5.0);
            p.rect(r.shrink(5.0, 5.0, 5.0, 5.0), Color::WHITE.with_alpha(0.9), 2.0);
        });
        ui.heading("libgui");
        ui.label_muted("hybrid UI scaffold");
        ui.flex();
        if ui.button("Reset view").clicked {
            d.reset_view();
        }
        let play = if d.playing { "Pause" } else { "Play" };
        if ui.button_primary(play).clicked {
            d.playing = !d.playing;
        }
    });

    // ── Body: outliner/inspector | viewport | stats ────────────────────────
    ui.container(Layout::row().height(Size::Grow(1.0)), Frame::none(), |ui| {
        let side = Layout::column().width(Size::Fixed(272.0)).padding(Insets::all(14.0)).gap(8.0);
        ui.container(side, Frame { border: Color::TRANSPARENT, ..Frame::panel(&t) }, |ui| {
            ui.section("Outliner");
            ui.text_input("search", &mut d.filter, "Search objects…");
            let filter = d.filter.to_lowercase();
            let list = ScrollOptions { gap: 1.0, padding: Insets { right: 10.0, ..Insets::all(0.0) }, ..ScrollOptions::new(Size::Fixed(232.0)) };
            let mut picked = None;
            ui.scroll_area_with("outliner", list, |ui| {
                for (i, name) in d.objects.iter().enumerate() {
                    if filter.is_empty() || name.to_lowercase().contains(&filter) {
                        if ui.selectable(name, d.selected == i).clicked {
                            picked = Some(i);
                        }
                    }
                }
            });
            if let Some(i) = picked {
                d.selected = i;
                let name = d.objects[i].clone();
                d.log(format!("selected {name}"));
            }
            ui.separator();
            // Inspector scrolls independently when the window is short.
            let insp = ScrollOptions { gap: 6.0, padding: Insets { right: 10.0, ..Insets::all(0.0) }, ..ScrollOptions::new(Size::Grow(1.0)) };
            ui.scroll_area_with("inspector", insp, |ui| {
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
            });
        });

        let area = Layout::column().padding(Insets::all(10.0));
        ui.container(area, Frame::none(), |ui| {
            // Immediate-mode overlay drawn on top of the engine image.
            let overlay = d.overlay;
            let stats = format!("{:.1} ms  ·  {:.0} fps", d.avg_ms(), 1000.0 / d.avg_ms().max(0.01));
            let title = d.objects[d.selected].clone();
            let resp = ui.viewport("main", viewport, move |p: &mut Painter, r: Rect| {
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
                let hint = "Drag to orbit  ·  Scroll to zoom";
                p.text(Vec2::new(r.x + 14.0, r.bottom() - 26.0), small, t.text_faint, hint);
            });
            viewport_rect = resp.rect;
            d.yaw += resp.drag_delta.x * 0.008;
            d.pitch = (d.pitch + resp.drag_delta.y * 0.006).clamp(-0.2, 1.3);
            d.distance = (d.distance - resp.scroll.y * 0.02).clamp(3.0, 20.0);
        });

        let stats = Layout::column().width(Size::Fixed(280.0)).padding(Insets::all(14.0)).gap(8.0);
        ui.container(stats, Frame { border: Color::TRANSPARENT, ..Frame::panel(&t) }, |ui| {
            ui.section("Frame");
            let ms = d.avg_ms();
            ui.text_with(&format!("{ms:.2} ms"), 22.0, t.text);
            let hist: Vec<f32> = d.frame_ms.iter().copied().collect();
            ui.plot("frame times", &hist, 33.3, 56.0);
            ui.label_muted(&format!("UI: {} instances · {} draw calls", d.ui_instances, d.ui_batches));
            ui.space(4.0);
            ui.section("Console");
            let well = Frame { fill: t.bg_inset, border: t.border, border_width: 1.0, radius: t.radius, shadow: false, clip: true };
            ui.container(Layout::column().height(Size::Grow(1.0)), well, |ui| {
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
                ui.set_focus(Some(cmd.response.id)); // keep typing, like a real console
            }
        });
    });

    // ── Status bar ─────────────────────────────────────────────────────────
    let status = Layout::row().height(Size::Fixed(26.0)).padding(Insets::xy(12.0, 0.0)).gap(12.0);
    ui.container(status, Frame { clip: false, ..Frame::panel(&t) }, |ui| {
        let (s, c) = (t.font_size_small, t.text_faint);
        ui.text_with(if d.playing { "● Playing" } else { "❚❚ Paused" }, s, c);
        ui.flex();
        ui.text_with("libgui 0.1  ·  wgpu backend", s, c);
    });

    viewport_rect
}

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: libgui_wgpu::Renderer,
    scene: Scene,
    viewport_tex: TextureId,
    ui: Ui,
    demo: Demo,
    input: Input,
    last: Instant,
    cursor: Cursor,
    clipboard: Option<arboard::Clipboard>,
}

impl Gpu {
    async fn new(el: &ActiveEventLoop, window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
            el.owned_display_handle(),
        )));
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("no GPU adapter");
        let (device, queue) = adapter.request_device(&Default::default()).await.expect("device");

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface unsupported");
        // Non-sRGB target: UI colours are authored in sRGB and blended like most
        // design tools expect.
        let caps = surface.get_capabilities(&adapter);
        config.format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let mut renderer = libgui_wgpu::Renderer::new(&device, &queue, config.format);
        let scene = Scene::new(&device);
        let viewport_tex = renderer.register_texture(&scene.color_view);
        let ui = Ui::new(libgui::Theme::dark(), include_bytes!("../../../assets/Inter.ttf"));

        Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            scene,
            viewport_tex,
            ui,
            demo: Demo::default(),
            input: Input::default(),
            last: Instant::now(),
            cursor: Cursor::Default,
            clipboard: arboard::Clipboard::new().ok(),
        }
    }

    fn scale(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    fn resize(&mut self) {
        let size = self.window.inner_size();
        self.config.width = size.width.max(1);
        self.config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.demo.frame_ms.push_back(dt * 1000.0);
        if self.demo.frame_ms.len() > 90 {
            self.demo.frame_ms.pop_front();
        }

        let scale = self.scale();
        self.input.dt = dt;
        self.input.scale = scale;
        self.input.screen_size = Vec2::new(self.config.width as f32 / scale, self.config.height as f32 / scale);

        // 1. Build UI (immediate API) and solve layout.
        let input = self.input.clone();
        self.input.scroll = Vec2::ZERO;
        self.input.events.clear();
        self.ui.begin_frame(input);
        let vp = build_ui(&mut self.ui, &mut self.demo, self.viewport_tex);
        if self.ui.cursor != self.cursor {
            self.cursor = self.ui.cursor;
            self.window.set_cursor(match self.cursor {
                Cursor::Default => CursorIcon::Default,
                Cursor::Pointer => CursorIcon::Pointer,
                Cursor::ResizeHorizontal => CursorIcon::EwResize,
                Cursor::Grab => CursorIcon::Grab,
                Cursor::Grabbing => CursorIcon::Grabbing,
                Cursor::Text => CursorIcon::Text,
            });
        }
        let out = self.ui.end_frame();
        self.demo.ui_instances = out.draw.instances.len();
        self.demo.ui_batches = out.draw.batches.len();

        // 2. Engine renders the viewport at exactly the size the UI gave it.
        if self.demo.playing && self.demo.auto_rotate {
            self.demo.spin += dt * self.demo.spin_speed;
        }
        let (w, h) = ((vp.w * scale).round() as u32, (vp.h * scale).round() as u32);
        if self.scene.resize(&self.device, w, h) {
            self.renderer.update_texture(self.viewport_tex, &self.scene.color_view);
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let d = &self.demo;
        let params = SceneParams { yaw: d.yaw, pitch: d.pitch, distance: d.distance, scale: d.scale, spin: d.spin };
        self.scene.render(&self.queue, &mut encoder, &params);

        // 3. UI composited into the swapchain.
        self.renderer.prepare(&out);
        let c = out.clear_color;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: c.r as f64, g: c.g as f64, b: c.b as f64, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut pass, &out);
        }
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.queue.present(frame);

        if let (Some(text), Some(cb)) = (self.ui.take_copied(), self.clipboard.as_mut()) {
            let _ = cb.set_text(text);
        }
    }

    /// Translate a winit key press into libgui events. Platform shortcuts are
    /// resolved here so the core stays OS-agnostic.
    fn key_event(&mut self, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let m = self.input.modifiers;
        let key = match &event.logical_key {
            WKey::Named(n) => match n {
                NamedKey::Backspace => Some(Key::Backspace),
                NamedKey::Delete => Some(Key::Delete),
                NamedKey::ArrowLeft => Some(Key::ArrowLeft),
                NamedKey::ArrowRight => Some(Key::ArrowRight),
                NamedKey::ArrowUp => Some(Key::ArrowUp),
                NamedKey::ArrowDown => Some(Key::ArrowDown),
                NamedKey::Home => Some(Key::Home),
                NamedKey::End => Some(Key::End),
                NamedKey::PageUp => Some(Key::PageUp),
                NamedKey::PageDown => Some(Key::PageDown),
                NamedKey::Enter => Some(Key::Enter),
                NamedKey::Escape => Some(Key::Escape),
                NamedKey::Tab => Some(Key::Tab),
                _ => None,
            },
            WKey::Character(c) if m.command => {
                match c.to_lowercase().as_str() {
                    "a" => return self.input.events.push(Event::Key(Key::A, m)),
                    "c" => self.input.events.push(Event::Copy),
                    "x" => self.input.events.push(Event::Cut),
                    "v" => {
                        if let Some(text) = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()) {
                            self.input.events.push(Event::Paste(text));
                        }
                    }
                    _ => {}
                }
                return;
            }
            _ => None,
        };
        match key {
            Some(k) => self.input.events.push(Event::Key(k, m)),
            None => {
                if let Some(text) = &event.text {
                    if !text.chars().all(char::is_control) {
                        self.input.events.push(Event::Text(text.to_string()));
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct App {
    gpu: Option<Gpu>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("libgui — hybrid UI demo")
            .with_inner_size(LogicalSize::new(1360.0, 820.0));
        let window = Arc::new(el.create_window(attrs).expect("window"));
        self.gpu = Some(pollster::block_on(Gpu::new(el, window)));
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.gpu.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => g.resize(),
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f32>(g.window.scale_factor());
                g.input.mouse_pos = Vec2::new(p.x, p.y);
                g.input.mouse_inside = true;
            }
            WindowEvent::CursorLeft { .. } => g.input.mouse_inside = false,
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                g.input.mouse_down = state == ElementState::Pressed;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                g.input.scroll += match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y) * 24.0,
                    MouseScrollDelta::PixelDelta(p) => {
                        let p = p.to_logical::<f32>(g.window.scale_factor());
                        Vec2::new(p.x, p.y)
                    }
                };
            }
            WindowEvent::ModifiersChanged(mods) => {
                let s = mods.state();
                let mac = cfg!(target_os = "macos");
                g.input.modifiers = Modifiers {
                    shift: s.shift_key(),
                    command: if mac { s.super_key() } else { s.control_key() },
                    word: if mac { s.alt_key() } else { s.control_key() },
                };
            }
            WindowEvent::KeyboardInput { event, .. } => g.key_event(&event),
            WindowEvent::RedrawRequested => g.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = &self.gpu {
            g.window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run");
}
