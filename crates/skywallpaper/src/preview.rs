use std::fs;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use sky_core::{SkyView, SkyWeather};
use sky_gpu::{SkyRenderer, SkyUniforms, pick_srgb_format};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::debug::DebugParams;

pub struct PreviewOpts {
    pub latitude: f64,
    pub longitude: f64,
    pub weather: SkyWeather,
    pub debug_path: Option<PathBuf>,
    pub debug_panel: Option<Child>,
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: SkyRenderer,
}

const PREVIEW_FRAME: Duration = Duration::from_millis(33);

struct PreviewApp {
    opts: PreviewOpts,
    window: Option<Arc<dyn Window>>,
    gpu: Option<Gpu>,
    start: Instant,
    last_redraw: Instant,
    thunder: f32,
    thunder_seed: f32,
    anim_hold: Option<f32>,
    debug_cache: Option<DebugParams>,
    debug_mtime: Option<SystemTime>,
    debug_panel: Option<Child>,
}

pub fn run(mut opts: PreviewOpts) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let debug_cache = opts
        .debug_path
        .as_ref()
        .and_then(|path| DebugParams::load(path));
    let debug_panel = opts.debug_panel.take();
    let app = PreviewApp {
        opts,
        window: None,
        gpu: None,
        start: Instant::now(),
        last_redraw: Instant::now(),
        thunder: 0.0,
        thunder_seed: 0.0,
        anim_hold: None,
        debug_cache,
        debug_mtime: None,
        debug_panel,
    };
    event_loop.run_app(app)?;
    Ok(())
}

impl ApplicationHandler for PreviewApp {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let title = if self.opts.debug_path.is_some() {
            "SkyWallpaper Debug"
        } else {
            "SkyWallpaper Preview"
        };
        let attrs = WindowAttributes::default().with_title(title);
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
                window.request_redraw();
                self.window = Some(window);
            }
            Err(err) => {
                eprintln!("preview gpu failed: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.teardown_debug();
                event_loop.exit();
            }
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

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let next = self.last_redraw + PREVIEW_FRAME;
        event_loop.set_control_flow(ControlFlow::WaitUntil(next));
        if Instant::now() >= next {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

impl Drop for PreviewApp {
    fn drop(&mut self) {
        self.teardown_debug();
    }
}

impl PreviewApp {
    fn teardown_debug(&mut self) {
        if let Some(mut child) = self.debug_panel.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(path) = self.opts.debug_path.take() {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(path.with_extension("tmp"));
        }
    }

    fn refresh_debug(&mut self) -> Option<DebugParams> {
        let path = self.opts.debug_path.as_ref()?;
        if let Ok(meta) = fs::metadata(path)
            && let Ok(mtime) = meta.modified()
            && self.debug_mtime != Some(mtime)
        {
            if let Some(params) = DebugParams::load(path) {
                self.debug_cache = Some(params);
            }
            self.debug_mtime = Some(mtime);
        }
        self.debug_cache
    }

    fn redraw(&mut self) {
        self.last_redraw = Instant::now();
        let elapsed = self.start.elapsed().as_secs_f32();
        let debug = self.refresh_debug();
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let (view, time, thunder, thunder_seed) = if let Some(params) = debug {
            let time = if params.anim_paused {
                *self.anim_hold.get_or_insert(elapsed)
            } else {
                self.anim_hold = None;
                elapsed
            };
            (
                params.build_view(),
                time,
                params.thunder.clamp(0.0, 1.0),
                0.0,
            )
        } else {
            if self.opts.weather.thunder && self.thunder < 0.05 && fastrand(elapsed) < 0.008 {
                self.thunder = 1.0;
                self.thunder_seed = elapsed;
            }
            self.thunder *= 0.82;
            self.anim_hold = None;
            (
                SkyView::now(self.opts.latitude, self.opts.longitude, self.opts.weather),
                elapsed,
                self.thunder,
                self.thunder_seed,
            )
        };
        let uniforms = SkyUniforms::from_flash(
            &view,
            gpu.config.width,
            gpu.config.height,
            time,
            thunder,
            thunder_seed,
        );
        gpu.renderer
            .retain_sizes(&[(gpu.config.width, gpu.config.height)]);
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
        let tex = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("preview"),
            });
        gpu.renderer.draw(
            &gpu.device,
            &mut encoder,
            &tex,
            gpu.config.width,
            gpu.config.height,
        );
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
