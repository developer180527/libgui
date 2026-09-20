//! A host for the second demo: one OS window per dock surface. The main window
//! holds the editor; every panel torn out of it gets a real window of its own,
//! and all of them share one wgpu device.
//!
//! The parts worth copying are the two that decide whether a resize feels
//! solid: the swapchain is reconfigured once per drawn frame rather than once
//! per resize event, and the frame gate is `needs_frame_for`, which sees a size
//! change. `needs_frame` alone does not — no input describes a resize — so a
//! host that gates on it re-presents batches built for the old window.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use libgui::*;
use libgui_solaris::App;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// GPU objects every window shares.
struct Gfx {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// One OS window = one dock surface.
struct Win {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: libgui_wgpu::Renderer,
    ui: Ui,
    platform: libgui_winit::PlatformState,
    dock_id: SurfaceId,
    last: Instant,
    idle: f32,
    visible: bool,
    title: String,
    /// Redrawn on the frames the UI did not need to run.
    batches: Vec<Batch>,
    clear: Color,
}

#[derive(Default)]
struct Host {
    gfx: Option<Gfx>,
    wins: HashMap<WindowId, Win>,
    app: App,
    /// Inner minus outer position: what `set_outer_position` has to undo.
    decoration: Vec2,
    /// Left button state across all windows — a drag can end in any of them.
    left_down: bool,
}

fn vec(p: PhysicalPosition<i32>) -> Vec2 {
    Vec2::new(p.x as f32, p.y as f32)
}

/// Top-left of the window's content in screen px, which is the space the dock
/// does its hit-testing in.
fn content_origin(w: &Window) -> Vec2 {
    w.inner_position().map(vec).unwrap_or(Vec2::ZERO)
}

impl Host {
    fn create_window(&mut self, el: &ActiveEventLoop, dock_id: SurfaceId, title: &str, size: Vec2, inner_pos: Option<Vec2>) -> WindowId {
        let main = dock_id == SurfaceId::MAIN;
        let mut attrs = Window::default_attributes()
            .with_title(title)
            // Don't steal focus mid-drag: the source window keeps the mouse.
            .with_active(main)
            .with_inner_size(LogicalSize::new(size.x, size.y));
        if let Some(p) = inner_pos {
            let outer = p - self.decoration;
            attrs = attrs.with_position(PhysicalPosition::new(outer.x as i32, outer.y as i32));
        }
        let window = Arc::new(el.create_window(attrs).expect("window"));

        if self.gfx.is_none() {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(el.owned_display_handle())));
            let probe = instance.create_surface(window.clone()).expect("surface");
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&probe),
                ..Default::default()
            }))
            .expect("no GPU adapter");
            let desc = wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..Default::default() };
            let (device, queue) = pollster::block_on(adapter.request_device(&desc)).expect("device");
            drop(probe);
            self.gfx = Some(Gfx { instance, adapter, device, queue });
        }
        let g = self.gfx.as_ref().unwrap();
        let surface = g.instance.create_surface(window.clone()).expect("surface");
        let px = window.inner_size();
        let mut config =
            surface.get_default_config(&g.adapter, px.width.max(1), px.height.max(1)).expect("surface config");
        let caps = surface.get_capabilities(&g.adapter);
        config.format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&g.device, &config);
        let renderer = libgui_wgpu::Renderer::new(&g.device, &g.queue, config.format);

        if main {
            if let (Ok(o), Ok(i)) = (window.outer_position(), window.inner_position()) {
                self.decoration = vec(i) - vec(o);
            }
        }

        // Every window has its own `Ui`, and each needs the keyboard convention.
        let mut ui = Ui::new(libgui_solaris::theme(), FONT).expect("bundled font");
        libgui_keymap::Keymap::<u8>::for_current_platform().install(&mut ui);
        ui.reserve(8_000);
        let clear = ui.theme.palette.bg_app;

        let id = window.id();
        self.wins.insert(
            id,
            Win {
                window,
                surface,
                config,
                renderer,
                ui,
                platform: libgui_winit::PlatformState::default(),
                dock_id,
                last: Instant::now(),
                idle: 0.0,
                visible: true,
                title: title.to_string(),
                batches: Vec::new(),
                clear,
            },
        );
        id
    }

    /// Tell the dock where each window's content sits, so a tab dragged out of
    /// one window lands in the right place in another.
    fn report_frame(&mut self, wid: WindowId) {
        if let Some(w) = self.wins.get(&wid) {
            self.app.dock.set_surface_frame(w.dock_id, content_origin(&w.window), w.window.scale_factor() as f32);
        }
    }

    /// Make the OS windows match the dock: create, destroy, move, show, hide.
    fn sync_windows(&mut self, el: &ActiveEventLoop) {
        // Windows whose surface is gone (the last tab was dragged out of it).
        let alive: Vec<SurfaceId> = self.app.dock.surfaces().iter().map(|s| s.id).collect();
        self.wins.retain(|_, w| alive.contains(&w.dock_id));

        // New floating surfaces. Draw each one immediately, so a torn-off panel
        // appears with its content rather than as an empty frame.
        let missing: Vec<(SurfaceId, String, Vec2, Option<Vec2>)> = self
            .app
            .dock
            .surfaces()
            .iter()
            .filter(|s| !self.wins.values().any(|w| w.dock_id == s.id))
            .map(|s| {
                (s.id, s.first_tab().map_or("libgui", |t| t.title()).to_string(), s.window_size, s.window_pos)
            })
            .collect();
        for (sid, title, size, pos) in missing {
            let wid = self.create_window(el, sid, &title, size, pos);
            self.report_frame(wid);
            self.draw(wid);
        }

        for w in self.wins.values_mut() {
            let Some(s) = self.app.dock.surface(w.dock_id) else { continue };
            // libgui moves a torn-off window while it is being dragged.
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
    }

    fn draw(&mut self, wid: WindowId) {
        // Taken out of the map so the dock (which lives in `self.app`) can be
        // borrowed while this window's `Ui` is.
        let Some(mut w) = self.wins.remove(&wid) else { return };
        let g = self.gfx.as_mut().expect("gpu");

        // The window may have changed size since the last frame, and on some
        // platforms (rotation, Stage Manager, split view) with no event at all.
        // One check here covers every case, and coalesces a whole resize drag
        // into a single reconfigure per drawn frame instead of one per event.
        let px = w.window.inner_size();
        if px.width.max(1) != w.config.width || px.height.max(1) != w.config.height {
            w.config.width = px.width.max(1);
            w.config.height = px.height.max(1);
            w.surface.configure(&g.device, &w.config);
        }

        let now = Instant::now();
        let dt = (now - w.last).as_secs_f32().min(0.1);
        w.last = now;
        w.idle += dt;

        let scale = w.window.scale_factor() as f32;
        let info = FrameInfo {
            screen_size: Vec2::new(w.config.width as f32 / scale, w.config.height as f32 / scale),
            scale,
            dt: w.idle,
        };

        // Only rebuild when the frame would come out different. `needs_frame_for`
        // rather than `needs_frame`: a resize pushes no input, so the size-blind
        // check would keep re-presenting batches built for the old window.
        let mut platform = PlatformOutput::default();
        if w.ui.needs_frame_for(&info, w.idle) {
            w.idle = 0.0;
            w.ui.begin_frame(info);
            self.app.ui_for(&mut w.ui, w.dock_id);
            let out = w.ui.end_frame();
            platform = out.platform.clone();
            w.clear = out.clear_color;
            w.renderer.prepare(&out);
            w.batches.clear();
            w.batches.extend_from_slice(&out.draw.batches);
        }

        let frame = match w.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                w.surface.configure(&g.device, &w.config);
                self.wins.insert(wid, w);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = g.device.create_command_encoder(&Default::default());
        {
            let c = w.clear;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: c.r as f64,
                            g: c.g as f64,
                            b: c.b as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            w.renderer.render_batches(&mut pass, &w.batches);
        }
        g.queue.submit([encoder.finish()]);
        w.window.pre_present_notify();
        g.queue.present(frame);
        w.platform.apply(&w.window, &platform);
        self.wins.insert(wid, w);
    }
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.wins.is_empty() {
            self.create_window(
                el,
                SurfaceId::MAIN,
                "libgui — Solaris-shaped editor",
                Vec2::new(1800.0, 1000.0),
                None,
            );
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
        let Some(w) = self.wins.get_mut(&wid) else { return };
        libgui_winit::push_window_event(&mut w.ui, &event, w.window.scale_factor());
        match event {
            WindowEvent::CloseRequested => {
                if w.dock_id == SurfaceId::MAIN {
                    el.exit();
                } else {
                    let id = w.dock_id;
                    self.app.dock.close_surface(id);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                // Only ask to draw. `draw` reconfigures the swapchain when it
                // and the window disagree, which turns a drag's worth of events
                // into one reconfigure per frame actually presented.
                w.window.request_redraw();
            }
            WindowEvent::RedrawRequested => self.draw(wid),
            WindowEvent::CursorMoved { position, .. } => {
                // The dock hit-tests in screen coordinates: during a drag the
                // source window keeps receiving moves even outside its bounds.
                if let Ok(origin) = w.window.inner_position() {
                    let screen = vec(origin) + Vec2::new(position.x as f32, position.y as f32);
                    self.app.dock.set_pointer(screen, self.left_down);
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let down = state == ElementState::Pressed;
                if down && self.app.dock.is_dragging() {
                    // A press while still "dragging" means we missed a release.
                    self.app.dock.cancel_drag();
                }
                self.left_down = down;
                self.app.dock.set_pointer_down(down);
                if !down {
                    // The drag may have ended over another window: release everywhere.
                    let up = InputEvent::PointerButton { button: PointerButton::Primary, pressed: false };
                    for (id, other) in self.wins.iter_mut() {
                        if *id != wid {
                            other.ui.push(up.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            return;
        }
        self.app.dock.update();
        self.sync_windows(el);
        for w in self.wins.values() {
            if w.visible {
                w.window.request_redraw();
            }
        }
    }
}

fn main() {
    let el = EventLoop::new().expect("event loop");
    el.run_app(&mut Host::default()).expect("run");
}
