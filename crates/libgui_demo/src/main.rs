//! Multi-window editor demo. One OS window per dock surface: the main window
//! plus a real OS window for every torn-off panel. All windows share one wgpu
//! device and the engine scene; each has its own surface, renderer and `Ui`.

mod panels;
mod scene;

use libgui::{Backend, Color, Cursor, DockState, Event, Frame, Input, Insets, Key, Layout, Modifiers, Size, SurfaceId, TextureId, Ui, Vec2};
use panels::{default_layout, Demo, Panels, Tab};
use scene::{Scene, SceneParams};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
/// All windows register the viewport texture under the same id.
const VIEWPORT_TEX: TextureId = TextureId::User(0);

/// GPU objects shared by every window.
struct Gfx {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    scene: Scene,
}

/// One OS window = one dock surface.
struct Win {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: libgui_wgpu::Renderer,
    ui: Ui,
    input: Input,
    cursor: Cursor,
    dock_id: SurfaceId,
    last: Instant,
    visible: bool,
    title: String,
}

struct App {
    gfx: Option<Gfx>,
    wins: HashMap<WindowId, Win>,
    dock: DockState<Tab>,
    demo: Demo,
    clipboard: Option<arboard::Clipboard>,
    /// Left button state across all windows (drags can end in any of them).
    left_down: bool,
    /// Outer-minus-inner offset of a decorated window (title bar), physical px.
    decoration: Vec2,
}

fn vec(p: PhysicalPosition<i32>) -> Vec2 {
    Vec2::new(p.x as f32, p.y as f32)
}

impl App {
    fn new() -> Self {
        let mut dock = DockState::new();
        default_layout(&mut dock);
        Self {
            gfx: None,
            wins: HashMap::new(),
            dock,
            demo: Demo::default(),
            clipboard: arboard::Clipboard::new().ok(),
            left_down: false,
            decoration: Vec2::ZERO,
        }
    }

    fn create_window(&mut self, el: &ActiveEventLoop, dock_id: SurfaceId, title: &str, size: Vec2, inner_pos: Option<Vec2>) -> WindowId {
        let main = dock_id == SurfaceId::MAIN;
        let mut attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(size.x, size.y))
            // Don't steal focus mid-drag: the source window keeps receiving the mouse.
            .with_active(main);
        if let Some(p) = inner_pos {
            let outer = p - self.decoration;
            attrs = attrs.with_position(PhysicalPosition::new(outer.x as i32, outer.y as i32));
        }
        let window = Arc::new(el.create_window(attrs).expect("window"));

        if self.gfx.is_none() {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(el.owned_display_handle())));
            let probe = instance.create_surface(window.clone()).expect("surface");
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&probe),
                ..Default::default()
            }))
            .expect("no GPU adapter");
            let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).expect("device");
            let scene = Scene::new(&device);
            drop(probe);
            self.gfx = Some(Gfx { instance, adapter, device, queue, scene });
        }
        let g = self.gfx.as_ref().unwrap();
        let surface = g.instance.create_surface(window.clone()).expect("surface");
        let px = window.inner_size();
        let mut config = surface.get_default_config(&g.adapter, px.width.max(1), px.height.max(1)).expect("surface config");
        let caps = surface.get_capabilities(&g.adapter);
        config.format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&g.device, &config);

        let mut renderer = libgui_wgpu::Renderer::new(&g.device, &g.queue, config.format);
        renderer.update_texture(VIEWPORT_TEX, &g.scene.color_view);

        if main {
            if let (Ok(o), Ok(i)) = (window.outer_position(), window.inner_position()) {
                self.decoration = vec(i) - vec(o);
            }
        }
        let id = window.id();
        self.wins.insert(
            id,
            Win {
                window,
                surface,
                config,
                renderer,
                ui: Ui::new(libgui::Theme::dark(), FONT),
                input: Input::default(),
                cursor: Cursor::Default,
                dock_id,
                last: Instant::now(),
                visible: true,
                title: title.to_string(),
            },
        );
        id
    }

    /// Make OS windows match the dock: create/destroy/move/show/hide.
    fn sync_windows(&mut self, el: &ActiveEventLoop) {
        self.dock.config = self.demo.dock_cfg.clone();
        if std::mem::take(&mut self.demo.reset_layout) {
            let cfg = self.dock.config.clone();
            self.dock = DockState::new();
            self.dock.config = cfg;
            default_layout(&mut self.dock);
        }

        // Destroy windows whose surface is gone.
        let alive: Vec<SurfaceId> = self.dock.surfaces().iter().map(|s| s.id).collect();
        self.wins.retain(|_, w| alive.contains(&w.dock_id));

        // Create windows for new floating surfaces, render them at once so they
        // appear with content instead of a blank frame.
        let missing: Vec<(SurfaceId, String, Vec2, Option<Vec2>)> = self
            .dock
            .surfaces()
            .iter()
            .filter(|s| !self.wins.values().any(|w| w.dock_id == s.id))
            .map(|s| (s.id, s.first_tab().map_or("libgui", |t| t.title()).to_string(), s.window_size, s.window_pos))
            .collect();
        for (sid, title, size, pos) in missing {
            let wid = self.create_window(el, sid, &title, size, pos);
            self.report_frame(wid);
            self.render(wid);
        }

        for w in self.wins.values_mut() {
            let Some(s) = self.dock.surface(w.dock_id) else { continue };
            if let Some(p) = s.window_pos {
                let outer = p - self.decoration;
                w.window.set_outer_position(PhysicalPosition::new(outer.x.round() as i32, outer.y.round() as i32));
            }
            if s.visible != w.visible {
                w.visible = s.visible;
                w.window.set_visible(s.visible);
            }
            let title = s.first_tab().map_or("libgui", |t| t.title());
            if w.dock_id != SurfaceId::MAIN && title != w.title {
                w.title = title.to_string();
                w.window.set_title(title);
            }
        }
        let ids: Vec<WindowId> = self.wins.keys().copied().collect();
        for wid in ids {
            self.report_frame(wid);
        }
        self.demo.windows = self.wins.len();
    }

    fn report_frame(&mut self, wid: WindowId) {
        if let Some(w) = self.wins.get(&wid) {
            if let Ok(p) = w.window.inner_position() {
                self.dock.set_surface_frame(w.dock_id, vec(p), w.window.scale_factor() as f32);
            }
        }
    }

    fn render(&mut self, wid: WindowId) {
        let Some(mut w) = self.wins.remove(&wid) else { return };
        let g = self.gfx.as_mut().unwrap();
        let main = w.dock_id == SurfaceId::MAIN;
        let now = Instant::now();
        let dt = (now - w.last).as_secs_f32().min(0.1);
        w.last = now;
        let scale = w.window.scale_factor() as f32;
        w.input.dt = dt;
        w.input.scale = scale;
        w.input.screen_size = Vec2::new(w.config.width as f32 / scale, w.config.height as f32 / scale);

        if main {
            let d = &mut self.demo;
            d.frame_ms.push_back(dt * 1000.0);
            if d.frame_ms.len() > 90 {
                d.frame_ms.pop_front();
            }
            if d.playing && d.auto_rotate {
                d.spin += dt * d.spin_speed;
            }
        }

        // 1. UI
        let input = w.input.clone();
        w.input.scroll = Vec2::ZERO;
        w.input.events.clear();
        w.ui.begin_frame(input);
        {
            let ui = &mut w.ui;
            if main {
                top_bar(ui, &mut self.demo);
            }
            let dock = &mut self.dock;
            let mut viewer = Panels { d: &mut self.demo, viewport_tex: VIEWPORT_TEX, scale };
            ui.container(Layout::column().shrink().padding(Insets::all(4.0)), Frame::none(), |ui| {
                dock.show(ui, w.dock_id, &mut viewer);
            });
            if main {
                status_bar(ui, &self.demo);
            }
        }
        if w.ui.cursor != w.cursor {
            w.cursor = w.ui.cursor;
            w.window.set_cursor(match w.cursor {
                Cursor::Default => CursorIcon::Default,
                Cursor::Pointer => CursorIcon::Pointer,
                Cursor::ResizeHorizontal => CursorIcon::EwResize,
                Cursor::ResizeVertical => CursorIcon::NsResize,
                Cursor::Grab => CursorIcon::Grab,
                Cursor::Grabbing => CursorIcon::Grabbing,
                Cursor::Text => CursorIcon::Text,
            });
        }
        let out = w.ui.end_frame();
        if main {
            self.demo.ui_instances = out.draw.instances.len();
            self.demo.ui_batches = out.draw.batches.len();
        }

        // 2. Engine: the main window drives the scene render at the size the
        //    viewport panel had, wherever it lives.
        let mut encoder = g.device.create_command_encoder(&Default::default());
        if main {
            let (vw, vh) = self.demo.viewport_px;
            if g.scene.resize(&g.device, vw, vh) {
                w.renderer.update_texture(VIEWPORT_TEX, &g.scene.color_view);
                for other in self.wins.values_mut() {
                    other.renderer.update_texture(VIEWPORT_TEX, &g.scene.color_view);
                }
            }
            let d = &self.demo;
            let params = SceneParams { yaw: d.yaw, pitch: d.pitch, distance: d.distance, scale: d.scale, spin: d.spin };
            g.scene.render(&g.queue, &mut encoder, &params);
        }

        // 3. Composite UI into this window's swapchain.
        let frame = match w.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Some(f),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                w.surface.configure(&g.device, &w.config);
                None
            }
            _ => None,
        };
        if let Some(frame) = frame {
            let view = frame.texture.create_view(&Default::default());
            w.renderer.prepare(&out);
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
                w.renderer.render(&mut pass, &out);
            }
            g.queue.submit([encoder.finish()]);
            w.window.pre_present_notify();
            g.queue.present(frame);
        } else {
            g.queue.submit([encoder.finish()]);
        }

        if let (Some(text), Some(cb)) = (w.ui.take_copied(), self.clipboard.as_mut()) {
            let _ = cb.set_text(text);
        }
        self.wins.insert(wid, w);
    }

    /// Translate a winit key press into libgui events for window `wid`.
    fn key_event(&mut self, wid: WindowId, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let Some(w) = self.wins.get_mut(&wid) else { return };
        let m = w.input.modifiers;
        let events = &mut w.input.events;
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
                    "a" => events.push(Event::Key(Key::A, m)),
                    "c" => events.push(Event::Copy),
                    "x" => events.push(Event::Cut),
                    "v" => {
                        if let Some(text) = self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()) {
                            events.push(Event::Paste(text));
                        }
                    }
                    _ => {}
                }
                return;
            }
            _ => None,
        };
        match key {
            Some(k) => events.push(Event::Key(k, m)),
            None => {
                if let Some(text) = &event.text {
                    if !text.chars().all(char::is_control) {
                        events.push(Event::Text(text.to_string()));
                    }
                }
            }
        }
    }
}

fn top_bar(ui: &mut Ui, d: &mut Demo) {
    let t = ui.theme.clone();
    let bar = Layout::row().height(Size::Fixed(44.0)).padding(Insets::xy(14.0, 0.0)).gap(8.0);
    ui.container(bar, Frame { clip: false, ..Frame::panel(&t) }, |ui| {
        let id = ui.make_id("logo");
        ui.add_leaf(id, Layout::leaf(Size::Fixed(18.0), Size::Fixed(18.0)), Vec2::ZERO, false, |p, r| {
            let t = p.theme;
            p.shadow(r, 5.0, 8.0, t.accent.with_alpha(0.5));
            p.rect(r, t.accent, 5.0);
            p.rect(r.shrink(5.0, 5.0, 5.0, 5.0), Color::WHITE.with_alpha(0.9), 2.0);
        });
        ui.heading("libgui");
        ui.label_muted("drag any tab out of its bar to tear it off");
        ui.flex();
        if ui.button("Reset layout").clicked {
            d.reset_layout = true;
        }
        if ui.button("Reset view").clicked {
            d.reset_view();
        }
        let play = if d.playing { "Pause" } else { "Play" };
        if ui.button_primary(play).clicked {
            d.playing = !d.playing;
        }
    });
}

fn status_bar(ui: &mut Ui, d: &Demo) {
    let t = ui.theme.clone();
    let status = Layout::row().height(Size::Fixed(26.0)).padding(Insets::xy(12.0, 0.0)).gap(12.0);
    ui.container(status, Frame { clip: false, ..Frame::panel(&t) }, |ui| {
        let (s, c) = (t.font_size_small, t.text_faint);
        ui.text_with(if d.playing { "● Playing" } else { "❚❚ Paused" }, s, c);
        ui.flex();
        ui.text_with(&format!("{} window(s)  ·  libgui 0.1  ·  wgpu backend", d.windows), s, c);
    });
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.wins.is_empty() {
            self.create_window(el, SurfaceId::MAIN, "libgui — docking demo", Vec2::new(1440.0, 880.0), None);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
        let Some(w) = self.wins.get_mut(&wid) else { return };
        match event {
            WindowEvent::CloseRequested => {
                if w.dock_id == SurfaceId::MAIN {
                    el.exit();
                } else {
                    self.dock.close_surface(w.dock_id);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                let size = w.window.inner_size();
                w.config.width = size.width.max(1);
                w.config.height = size.height.max(1);
                w.surface.configure(&self.gfx.as_ref().unwrap().device, &w.config);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = w.window.scale_factor();
                let p = position.to_logical::<f32>(scale);
                w.input.mouse_pos = Vec2::new(p.x, p.y);
                w.input.mouse_inside = true;
                // Global pointer for docking. During a drag the source window keeps
                // receiving moves even outside its bounds.
                if let Ok(origin) = w.window.inner_position() {
                    let screen = vec(origin) + Vec2::new(position.x as f32, position.y as f32);
                    self.dock.set_pointer(screen, self.left_down);
                }
            }
            WindowEvent::CursorLeft { .. } => w.input.mouse_inside = false,
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let down = state == ElementState::Pressed;
                if down && self.dock.is_dragging() {
                    // A press while "dragging" means we missed the release.
                    self.dock.cancel_drag();
                }
                w.input.mouse_down = down;
                self.left_down = down;
                self.dock.set_pointer_down(down);
                if !down {
                    // A drag may have moved focus between windows; release everywhere.
                    for other in self.wins.values_mut() {
                        other.input.mouse_down = false;
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                w.input.scroll += match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y) * 24.0,
                    MouseScrollDelta::PixelDelta(p) => {
                        let p = p.to_logical::<f32>(w.window.scale_factor());
                        Vec2::new(p.x, p.y)
                    }
                };
            }
            WindowEvent::ModifiersChanged(mods) => {
                let s = mods.state();
                let mac = cfg!(target_os = "macos");
                w.input.modifiers = Modifiers {
                    shift: s.shift_key(),
                    command: if mac { s.super_key() } else { s.control_key() },
                    word: if mac { s.alt_key() } else { s.control_key() },
                };
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let esc = event.state == ElementState::Pressed && event.logical_key == WKey::Named(NamedKey::Escape);
                if esc && self.dock.is_dragging() {
                    self.dock.cancel_drag();
                } else {
                    self.key_event(wid, &event);
                }
            }
            WindowEvent::RedrawRequested => self.render(wid),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            return;
        }
        self.dock.update();
        self.sync_windows(el);
        for w in self.wins.values() {
            if w.visible {
                w.window.request_redraw();
            }
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run");
}
