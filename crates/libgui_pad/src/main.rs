//! A host for Pad: one window, one `Ui`, one wgpu surface.
//!
//! The other demos host a dock and put every torn-off panel in its own OS
//! window. This one is the short version — the smallest thing that runs a
//! libgui app — and it is worth reading for exactly that reason: a window, a
//! surface, `begin_frame` / build / `end_frame`, `prepare`, `render_batches`.
//!
//! The two details that are easy to get wrong are here anyway: the swapchain
//! is reconfigured once per *drawn frame* rather than once per resize event,
//! and the frame gate is `needs_frame_for`, which notices a size change —
//! `needs_frame` alone cannot, because no input describes a resize.

use std::sync::Arc;
use std::time::Instant;

use libgui::*;
use libgui_pad::Pad;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

struct Live {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: libgui_wgpu::Renderer,
    ui: Ui,
    platform: libgui_winit::PlatformState,
    last: Instant,
    /// Time since the last frame that was actually built.
    idle: f32,
    batches: Vec<Batch>,
    clear: Color,
}

#[derive(Default)]
struct Host {
    live: Option<Live>,
    pad: Pad,
    clipboard: Option<arboard::Clipboard>,
}

impl Host {
    fn start(&mut self, el: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("libgui — Pad")
            .with_inner_size(LogicalSize::new(1320.0, 900.0));
        let window = Arc::new(el.create_window(attrs).expect("window"));

        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(el.owned_display_handle())));
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no GPU adapter");
        let desc = wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..Default::default() };
        let (device, queue) = pollster::block_on(adapter.request_device(&desc)).expect("device");

        let px = window.inner_size();
        let mut config = surface.get_default_config(&adapter, px.width.max(1), px.height.max(1)).expect("config");
        let caps = surface.get_capabilities(&adapter);
        config.format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        let renderer = libgui_wgpu::Renderer::new(&device, &queue, config.format);

        // The library's half of the keyboard convention. Pad's own chords are
        // its keymap's, and live in the app.
        let mut ui = Ui::new(libgui_pad::theme(false), FONT).expect("bundled font");
        libgui_keymap::Keymap::<u8>::for_current_platform().install(&mut ui);
        ui.reserve(4_000);
        let clear = ui.theme.palette.bg_app;

        self.clipboard = arboard::Clipboard::new().ok();
        self.live = Some(Live {
            window,
            surface,
            config,
            device,
            queue,
            renderer,
            ui,
            platform: libgui_winit::PlatformState::default(),
            last: Instant::now(),
            idle: 0.0,
            batches: Vec::new(),
            clear,
        });
    }

    fn draw(&mut self) {
        let Some(w) = self.live.as_mut() else { return };

        // The window may have changed size since the last frame, and on some
        // platforms with no event at all. One check here covers every case and
        // turns a whole resize drag into one reconfigure per presented frame.
        let px = w.window.inner_size();
        if px.width.max(1) != w.config.width || px.height.max(1) != w.config.height {
            w.config.width = px.width.max(1);
            w.config.height = px.height.max(1);
            w.surface.configure(&w.device, &w.config);
        }

        let now = Instant::now();
        w.idle += (now - w.last).as_secs_f32().min(0.1);
        w.last = now;

        let scale = w.window.scale_factor() as f32;
        let info = FrameInfo {
            screen_size: Vec2::new(w.config.width as f32 / scale, w.config.height as f32 / scale),
            scale,
            dt: w.idle,
        };

        let mut platform = PlatformOutput::default();
        if w.ui.needs_frame_for(&info, w.idle) {
            w.idle = 0.0;
            w.ui.begin_frame(info);
            self.pad.ui(&mut w.ui);
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
                w.surface.configure(&w.device, &w.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = w.device.create_command_encoder(&Default::default());
        {
            let c = w.clear;
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
            w.renderer.render_batches(&mut pass, &w.batches);
        }
        w.queue.submit([encoder.finish()]);
        w.window.pre_present_notify();
        w.queue.present(frame);

        // The host's half of the UI's requests: cursor, IME, and the
        // clipboard — which a text editor leans on, and which libgui asks for
        // rather than touching itself.
        w.platform.apply(&w.window, &platform);
        if let Some(text) = platform.copied_text {
            if let Some(c) = self.clipboard.as_mut() {
                let _ = c.set_text(text);
            }
        }
        if platform.paste_requested {
            if let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) {
                w.ui.push(InputEvent::Paste(text));
            }
        }
    }
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.live.is_none() {
            self.start(el);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(w) = self.live.as_mut() else { return };
        libgui_winit::push_window_event(&mut w.ui, &event, w.window.scale_factor());
        match event {
            WindowEvent::CloseRequested => el.exit(),
            // Only ask to draw: `draw` reconfigures when the swapchain and the
            // window disagree.
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => w.window.request_redraw(),
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = self.live.as_ref() {
            w.window.request_redraw();
        }
    }
}

fn main() {
    let el = EventLoop::new().expect("event loop");
    el.run_app(&mut Host::default()).expect("run");
}
