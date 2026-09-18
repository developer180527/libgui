//! Multi-window editor demo. One OS window per dock surface: the main window
//! plus a real OS window for every torn-off panel. All windows share one wgpu
//! device and the engine scene; each has its own surface, renderer and `Ui`.

mod panels;
mod scene;

use libgui::{
    Backend, Color, Density, DockState, FloatingMode, FrameInfo, Frame, InputEvent, Insets, Layout, PointerButton, Size, SurfaceId,
    TextureId, Theme, ThemeWatcher, Ui, Vec2,
};
use panels::{default_layout, Demo, Panels, Tab, THEMES};
use scene::{Scene, SceneParams};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{Window, WindowId};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const IOS: bool = cfg!(target_os = "ios");

/// Theme files compiled in, for devices where `themes/` isn't on disk.
fn embedded_theme(file: &str) -> Option<&'static str> {
    match file {
        "unity.toml" => Some(include_str!("../../../themes/unity.toml")),
        "blender.toml" => Some(include_str!("../../../themes/blender.toml")),
        "custom.toml" => Some(include_str!("../../../themes/custom.toml")),
        _ => None,
    }
}

/// OS clipboard where available (desktop); a no-op elsewhere for now.
struct Clipboard(#[cfg(not(target_os = "ios"))] Option<arboard::Clipboard>);

impl Clipboard {
    fn new() -> Self {
        #[cfg(not(target_os = "ios"))]
        return Clipboard(arboard::Clipboard::new().ok());
        #[cfg(target_os = "ios")]
        Clipboard()
    }

    fn get(&mut self) -> Option<String> {
        #[cfg(not(target_os = "ios"))]
        return self.0.as_mut().and_then(|c| c.get_text().ok());
        #[cfg(target_os = "ios")]
        None
    }

    fn set(&mut self, _text: String) {
        #[cfg(not(target_os = "ios"))]
        if let Some(c) = self.0.as_mut() {
            let _ = c.set_text(_text);
        }
    }
}

/// Size of the drawable: iOS reports the safe area as the inner size, but the
/// Metal layer covers the whole screen.
fn surface_size(w: &Window) -> winit::dpi::PhysicalSize<u32> {
    if IOS {
        w.outer_size()
    } else {
        w.inner_size()
    }
}

/// Screen position of the drawable's top-left (physical px).
fn content_origin(w: &Window) -> Vec2 {
    let p = if IOS { w.outer_position() } else { w.inner_position() };
    p.map(vec).unwrap_or(Vec2::ZERO)
}

/// Safe-area insets in logical px (notch, home indicator, status bar).
fn safe_insets(w: &Window) -> Insets {
    if !IOS {
        return Insets::all(0.0);
    }
    let s = w.scale_factor() as f32;
    let (Ok(outer), Ok(inner)) = (w.outer_position(), w.inner_position()) else { return Insets::all(0.0) };
    let (os, is) = (w.outer_size(), w.inner_size());
    let left = (inner.x - outer.x) as f32 / s;
    let top = (inner.y - outer.y) as f32 / s;
    Insets {
        left,
        top,
        right: (os.width as f32 - is.width as f32) / s - left,
        bottom: (os.height as f32 - is.height as f32) / s - top,
    }
}
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
    /// Applies the UI's cursor / pointer-lock / keyboard requests to the window.
    platform: libgui_winit::PlatformState,
    dock_id: SurfaceId,
    last: Instant,
    visible: bool,
    title: String,
    /// Fingers down (id, logical pos): the dock follows the first one.
    fingers: Vec<(u64, Vec2)>,
}

struct App {
    gfx: Option<Gfx>,
    wins: HashMap<WindowId, Win>,
    dock: DockState<Tab>,
    demo: Demo,
    clipboard: Clipboard,
    /// Left button state across all windows (drags can end in any of them).
    left_down: bool,
    /// Outer-minus-inner offset of a decorated window (title bar), physical px.
    decoration: Vec2,
    /// One theme for every window; hot-reloaded from `themes/*.toml`.
    theme: Theme,
    watcher: Option<ThemeWatcher>,
    applied_theme: Option<(usize, usize)>,
}

fn themes_dir() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
    dir.canonicalize().unwrap_or(dir)
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
            clipboard: Clipboard::new(),
            left_down: false,
            decoration: Vec2::ZERO,
            theme: Theme::dark(),
            watcher: None,
            applied_theme: None,
        }
    }

    fn create_window(&mut self, el: &ActiveEventLoop, dock_id: SurfaceId, title: &str, size: Vec2, inner_pos: Option<Vec2>) -> WindowId {
        let main = dock_id == SurfaceId::MAIN;
        let mut attrs = Window::default_attributes()
            .with_title(title)
            // Don't steal focus mid-drag: the source window keeps receiving the mouse.
            .with_active(main);
        // iOS applies a requested size to the UIWindow frame; let it fill the screen.
        if !IOS {
            attrs = attrs.with_inner_size(LogicalSize::new(size.x, size.y));
        }
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
            // Ask for what this GPU supports: e.g. the iOS Simulator allows fewer
            // inter-stage variables than wgpu's desktop defaults.
            let desc = wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..Default::default() };
            let (device, queue) = pollster::block_on(adapter.request_device(&desc)).expect("device");
            let scene = Scene::new(&device);
            drop(probe);
            self.gfx = Some(Gfx { instance, adapter, device, queue, scene });
        }
        let g = self.gfx.as_ref().unwrap();
        let surface = g.instance.create_surface(window.clone()).expect("surface");
        let px = surface_size(&window);
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
                ui: Ui::new(libgui::Theme::dark(), FONT).expect("bundled font"),
                platform: libgui_winit::PlatformState::default(),
                dock_id,
                last: Instant::now(),
                visible: true,
                title: title.to_string(),
                fingers: Vec::new(),
            },
        );
        id
    }

    /// Apply the Appearance panel's choice, and hot-reload the watched file.
    fn update_theme(&mut self) {
        let d = &mut self.demo;
        let density = match d.density_choice {
            1 => Some(Density::Compact),
            2 => Some(Density::Regular),
            3 => Some(Density::Touch),
            _ => None,
        };
        let choice = (d.theme_choice, d.density_choice);
        if self.applied_theme != Some(choice) {
            self.applied_theme = Some(choice);
            match THEMES[d.theme_choice] {
                (_, Some(file)) if themes_dir().is_dir() => {
                    let mut w = ThemeWatcher::new(themes_dir().join(file));
                    w.density = density;
                    self.watcher = Some(w);
                }
                (_, Some(file)) => {
                    // On a device the source tree isn't there: use the compiled-in copy.
                    self.watcher = None;
                    match embedded_theme(file).map(|src| Theme::from_toml_with(src, density)) {
                        Some(Ok(t)) => {
                            d.theme_status = format!("Embedded {file} (hot reload needs the source tree)");
                            d.theme_error = false;
                            self.theme = t;
                        }
                        Some(Err(e)) => {
                            d.theme_status = e.to_string();
                            d.theme_error = true;
                        }
                        None => {}
                    }
                }
                (name, None) => {
                    self.watcher = None;
                    let mut t = Theme::preset(name).unwrap_or_default();
                    if let Some(dn) = density {
                        t.set_density(dn);
                    }
                    d.theme_status = format!("Built-in preset: {} ({:?})", t.name, t.density);
                    d.theme_error = false;
                    self.theme = t;
                }
            }
        }
        if let Some(w) = &mut self.watcher {
            if let Some(result) = w.poll() {
                let file = w.path().file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                match result {
                    Ok(t) => {
                        d.log(format!("theme reloaded: {} from {file}", t.name));
                        d.theme_status = format!("Watching themes/{file}: edit & save to reload");
                        d.theme_error = false;
                        self.theme = t;
                    }
                    Err(e) => {
                        d.log(format!("error: {e}"));
                        d.theme_status = format!("{e}  (keeping previous theme)");
                        d.theme_error = true;
                    }
                }
            }
        }
        if std::mem::take(&mut d.export_theme) {
            let path = themes_dir().join("_exported.toml");
            let body = format!("# Every resolved value of \"{}\". Copy the keys you want to change.\n{}", self.theme.name, self.theme.to_toml());
            match std::fs::write(&path, body) {
                Ok(()) => d.log(format!("exported theme to {}", path.display())),
                Err(e) => d.log(format!("error: export failed: {e}")),
            }
        }
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
        // appear with content instead of a blank frame. (In-app mode draws them
        // inside the main window instead.)
        let os_windows = self.dock.config.floating_mode == FloatingMode::OsWindows;
        let missing: Vec<(SurfaceId, String, Vec2, Option<Vec2>)> = self
            .dock
            .surfaces()
            .iter()
            .filter(|s| os_windows && !self.wins.values().any(|w| w.dock_id == s.id))
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
            self.dock.set_surface_frame(w.dock_id, content_origin(&w.window), w.window.scale_factor() as f32);
        }
    }

    fn render(&mut self, wid: WindowId) {
        let Some(mut w) = self.wins.remove(&wid) else { return };
        let g = self.gfx.as_mut().unwrap();
        let main = w.dock_id == SurfaceId::MAIN;
        // Rotation / Stage Manager / split view can change the size without a
        // Resized event on some platforms: keep the swapchain matching the window.
        let size = surface_size(&w.window);
        if size.width.max(1) != w.config.width || size.height.max(1) != w.config.height {
            w.config.width = size.width.max(1);
            w.config.height = size.height.max(1);
            w.surface.configure(&g.device, &w.config);
        }
        let now = Instant::now();
        let dt = (now - w.last).as_secs_f32().min(0.1);
        w.last = now;
        let scale = w.window.scale_factor() as f32;
        let info = FrameInfo { screen_size: Vec2::new(w.config.width as f32 / scale, w.config.height as f32 / scale), scale, dt };

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
        if w.ui.theme != self.theme {
            w.ui.theme = self.theme.clone();
        }
        w.ui.begin_frame(info);
        {
            let ui = &mut w.ui;
            let dock = &mut self.dock;
            let demo = &mut self.demo;
            let dock_id = w.dock_id;
            let safe = safe_insets(&w.window);
            ui.container(Layout::column().shrink().padding(safe), Frame::none(), |ui| {
                if main {
                    top_bar(ui, demo);
                }
                let mut viewer = Panels { d: &mut *demo, viewport_tex: VIEWPORT_TEX, scale };
                ui.container(Layout::column().shrink().padding(Insets::all(4.0)), Frame::none(), |ui| {
                    dock.show(ui, dock_id, &mut viewer);
                });
                if main {
                    status_bar(ui, demo);
                }
            });
        }
        let out = w.ui.end_frame();
        let platform = out.platform.clone();
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

        // Host side of the UI's requests: cursor, pointer lock, keyboard, clipboard.
        w.platform.apply(&w.window, &platform);
        if let Some(text) = platform.copied_text {
            self.clipboard.set(text);
        }
        if platform.paste_requested {
            if let Some(text) = self.clipboard.get() {
                w.ui.push(InputEvent::Paste(text));
            }
        }
        self.wins.insert(wid, w);
    }
}

fn top_bar(ui: &mut Ui, d: &mut Demo) {
    let t = ui.theme.clone();
    let bar = Layout::row().height(Size::Fixed(44.0)).padding(Insets::xy(14.0, 0.0)).gap(8.0);
    ui.container(bar, Frame { clip: false, ..Frame::panel(&t) }, |ui| {
        let id = ui.make_id("logo");
        ui.add_leaf(id, Layout::leaf(Size::Fixed(18.0), Size::Fixed(18.0)), Vec2::ZERO, false, |p, r| {
            let t = p.theme;
            p.shadow(r, 5.0, 8.0, t.palette.accent.with_alpha(0.5));
            p.rect(r, t.palette.accent, 5.0);
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
        let (s, c) = (t.metrics.font_size_small, t.palette.text_faint);
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
        // All input goes to this window's UI; the arms below are host-only concerns.
        libgui_winit::push_window_event(&mut w.ui, &event, w.window.scale_factor());
        match event {
            WindowEvent::CloseRequested => {
                if w.dock_id == SurfaceId::MAIN {
                    el.exit();
                } else {
                    self.dock.close_surface(w.dock_id);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                let size = surface_size(&w.window);
                w.config.width = size.width.max(1);
                w.config.height = size.height.max(1);
                w.surface.configure(&self.gfx.as_ref().unwrap().device, &w.config);
            }
            WindowEvent::CursorMoved { position, .. } => {
                // Global pointer for docking. During a drag the source window keeps
                // receiving moves even outside its bounds.
                if let Ok(origin) = w.window.inner_position() {
                    let screen = vec(origin) + Vec2::new(position.x as f32, position.y as f32);
                    self.dock.set_pointer(screen, self.left_down);
                }
            }
            WindowEvent::Touch(t) => {
                // The dock follows the first finger in screen coordinates.
                let scale = w.window.scale_factor() as f32;
                let pos = Vec2::new(t.location.x as f32 / scale, t.location.y as f32 / scale);
                match t.phase {
                    winit::event::TouchPhase::Started => w.fingers.push((t.id, pos)),
                    winit::event::TouchPhase::Moved => {
                        if let Some(f) = w.fingers.iter_mut().find(|f| f.0 == t.id) {
                            f.1 = pos;
                        }
                    }
                    _ => w.fingers.retain(|f| f.0 != t.id),
                }
                match w.fingers.first() {
                    Some(&(_, p)) => self.dock.set_pointer(content_origin(&w.window) + p * scale, true),
                    None => self.dock.set_pointer_down(false),
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let down = state == ElementState::Pressed;
                if down && self.dock.is_dragging() {
                    // A press while "dragging" means we missed the release.
                    self.dock.cancel_drag();
                }
                self.left_down = down;
                self.dock.set_pointer_down(down);
                if !down {
                    // A drag may have ended over another window: release everywhere.
                    let up = InputEvent::PointerButton { button: PointerButton::Primary, pressed: false };
                    for (id, other) in self.wins.iter_mut() {
                        if *id != wid {
                            other.ui.push(up.clone());
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let esc = event.state == ElementState::Pressed && event.logical_key == WKey::Named(NamedKey::Escape);
                if esc && self.dock.is_dragging() {
                    self.dock.cancel_drag();
                }
            }
            WindowEvent::RedrawRequested => self.render(wid),
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: winit::event::DeviceId, event: winit::event::DeviceEvent) {
        // Raw, unaccelerated motion. Only used while a widget holds pointer lock.
        for w in self.wins.values_mut() {
            libgui_winit::push_device_event(&mut w.ui, &event);
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            return;
        }
        self.dock.update();
        self.update_theme();
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
    // Tablets: floating panels live inside the app, and controls are touch-sized.
    // LIBGUI_INAPP=1 tries the tablet docking model on desktop.
    if IOS || std::env::var("LIBGUI_INAPP").is_ok_and(|v| v == "1") {
        app.demo.dock_cfg.floating_mode = FloatingMode::InApp;
    }
    if IOS {
        app.demo.density_choice = 3;
    }
    // Optional startup look: LIBGUI_THEME=dark|midnight|light|unity|blender|custom,
    // LIBGUI_DENSITY=compact|regular|touch.
    if let Ok(name) = std::env::var("LIBGUI_THEME") {
        let name = name.to_lowercase();
        if let Some(i) = THEMES.iter().position(|(label, _)| label.to_lowercase().starts_with(&name)) {
            app.demo.theme_choice = i;
        }
    }
    if let Ok(d) = std::env::var("LIBGUI_DENSITY") {
        app.demo.density_choice = ["theme", "compact", "regular", "touch"].iter().position(|x| *x == d.to_lowercase()).unwrap_or(0);
    }
    event_loop.run_app(&mut app).expect("run");
}
