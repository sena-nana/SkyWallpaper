use std::sync::Arc;
use std::time::Instant;

use sky_core::{SkyView, SkyWeather};
use sky_gpu::{SkyRenderer, SkyUniforms, pick_srgb_format};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

pub struct PreviewOpts {
    pub latitude: f64,
    pub longitude: f64,
    pub weather: SkyWeather,
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: SkyRenderer,
}

struct PreviewApp {
    opts: PreviewOpts,
    window: Option<Arc<dyn Window>>,
    gpu: Option<Gpu>,
    start: Instant,
    thunder: f32,
}

pub fn run(opts: PreviewOpts) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let app = PreviewApp {
        opts,
        window: None,
        gpu: None,
        start: Instant::now(),
        thunder: 0.0,
    };
    event_loop.run_app(app)?;
    Ok(())
}

impl ApplicationHandler for PreviewApp {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default().with_title("SkyWallpaper Preview");
        let window: Arc<dyn Window> = match event_loop.create_window(attrs) {
            Ok(window) => Arc::from(window),
            Err(err) => {
                eprintln!("preview window failed: {err}");
                event_loop.exit();
                return;
            }
        };
        match init_gpu(window.clone()) {
            Ok(gpu) => {
                self.gpu = Some(gpu);
                self.window = Some(window);
            }
            Err(err) => {
                eprintln!("preview gpu failed: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::SurfaceResized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl PreviewApp {
    fn redraw(&mut self) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        if self.opts.weather.thunder && fastrand(self.start.elapsed().as_secs_f32()) < 0.008 {
            self.thunder = 1.0;
        }
        self.thunder *= 0.82;
        let view = SkyView::now(self.opts.latitude, self.opts.longitude, self.opts.weather);
        let uniforms = SkyUniforms::from_view(
            &view,
            gpu.config.width,
            gpu.config.height,
            self.start.elapsed().as_secs_f32(),
            self.thunder,
        );
        gpu.renderer.write_uniforms(&gpu.queue, &uniforms);
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
            _ => return,
        };
        let tex = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("preview"),
            });
        gpu.renderer.draw(&mut encoder, &tex);
        gpu.queue.submit(Some(encoder.finish()));
        gpu.queue.present(frame);
    }
}

fn init_gpu(window: Arc<dyn Window>) -> anyhow::Result<Gpu> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let surface = instance.create_surface(window.clone())?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: Some(&surface),
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("preview"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))?;
    let caps = surface.get_capabilities(&adapter);
    let format = pick_srgb_format(&caps.formats);
    let size = window.surface_size();
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        color_space: wgpu::SurfaceColorSpace::Srgb,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&device, &config);
    let renderer = SkyRenderer::new(&device, format);
    Ok(Gpu {
        surface,
        device,
        queue,
        config,
        renderer,
    })
}

fn fastrand(t: f32) -> f32 {
    let n = (t * 12.9898).sin() * 43758.5453;
    n.fract().abs()
}
