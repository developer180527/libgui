//! A slim host for the second demo: one window, one `Ui`, and the smallest
//! winit + wgpu loop that draws it. The first demo's host carries docking
//! tear-off, multiple windows, theme hot-reload and a 3D scene; none of that
//! is needed to show a dense editor, and leaving it out keeps this file the
//! size a host adapter actually is.

use std::sync::Arc;

use libgui::*;
use libgui_solaris::App;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: libgui_wgpu::Renderer,
}

#[derive(Default)]
struct Host {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    ui: Option<Ui>,
    app: App,
    platform: libgui_winit::PlatformState,
    /// Draws the batches again on the frames the UI did not need to run.
    batches: Vec<Batch>,
    clear: Color,
    idle: f32,
    last: Option<std::time::Instant>,
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("libgui — Solaris-shaped editor")
            .with_inner_size(LogicalSize::new(1800.0, 1000.0));
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

        let size = window.inner_size();
        let mut config =
            surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).expect("surface config");
        let caps = surface.get_capabilities(&adapter);
        config.format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        let renderer = libgui_wgpu::Renderer::new(&device, &queue, config.format);

        let mut ui = Ui::new(libgui_solaris::theme(), FONT).expect("bundled font");
        // Both halves of the platform's keyboard convention.
        libgui_keymap::Keymap::<u8>::for_current_platform().install(&mut ui);
        ui.reserve(8_000);

        self.clear = ui.theme.palette.bg_app;
        self.ui = Some(ui);
        self.gpu = Some(Gpu { device, queue, surface, config, renderer });
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(window), Some(ui)) = (self.window.as_ref(), self.ui.as_mut()) else { return };
        libgui_winit::push_window_event(ui, &event, window.scale_factor());
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                let g = self.gpu.as_mut().expect("gpu");
                let px = window.inner_size();
                g.config.width = px.width.max(1);
                g.config.height = px.height.max(1);
                g.surface.configure(&g.device, &g.config);
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl Host {
    fn draw(&mut self) {
        let (Some(window), Some(ui), Some(g)) = (self.window.as_ref(), self.ui.as_mut(), self.gpu.as_mut()) else {
            return;
        };
        let now = std::time::Instant::now();
        let dt = self.last.replace(now).map_or(1.0 / 60.0, |t| (now - t).as_secs_f32().min(0.1));
        self.idle += dt;

        // Only rebuild when the frame would come out different; the rest of
        // the time the batches already uploaded are still correct.
        let mut platform = PlatformOutput::default();
        if ui.needs_frame(self.idle) {
            let scale = window.scale_factor() as f32;
            let info = FrameInfo {
                screen_size: Vec2::new(g.config.width as f32 / scale, g.config.height as f32 / scale),
                scale,
                dt: self.idle,
            };
            self.idle = 0.0;
            ui.begin_frame(info);
            self.app.ui(ui);
            let out = ui.end_frame();
            platform = out.platform.clone();
            self.clear = out.clear_color;
            g.renderer.prepare(&out);
            self.batches.clear();
            self.batches.extend_from_slice(&out.draw.batches);
        }

        let frame = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                g.surface.configure(&g.device, &g.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = g.device.create_command_encoder(&Default::default());
        {
            let c = self.clear;
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
            g.renderer.render_batches(&mut pass, &self.batches);
        }
        g.queue.submit([encoder.finish()]);
        window.pre_present_notify();
        g.queue.present(frame);
        self.platform.apply(window, &platform);
    }
}

fn main() {
    let el = EventLoop::new().expect("event loop");
    el.run_app(&mut Host::default()).expect("run");
}
