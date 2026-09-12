use std::num::NonZeroIsize;
use std::time::Instant;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use sky_core::SkyView;
use sky_gpu::{SkyRenderer, SkyUniforms, pick_srgb_format};
use wgpu::SurfaceTargetUnsafe;
use windows::Win32::Foundation::HWND;

use crate::workerw::{
    WorkerWError, create_monitor_window, destroy_hwnd, list_monitors_relative_to,
    register_surface_class, spawn_worker_w,
};

pub struct WallpaperEngine {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: wgpu::Adapter,
    renderer: Option<SkyRenderer>,
    slots: Vec<SurfaceSlot>,
    parent: HWND,
    start: Instant,
    pub thunder_flash: f32,
    pub thunder_seed: f32,
}

struct SurfaceSlot {
    hwnd: HWND,
    width: u32,
    height: u32,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

struct WallpaperHwnd {
    hwnd: HWND,
}

impl HasWindowHandle for WallpaperHwnd {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = self.hwnd.0 as isize;
        let nz = NonZeroIsize::new(hwnd).ok_or(HandleError::Unavailable)?;
        let handle = Win32WindowHandle::new(nz);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for WallpaperHwnd {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Windows(WindowsDisplayHandle::new()))
        })
    }
}

impl WallpaperEngine {
    pub fn attach() -> Result<Self, EngineError> {
        register_surface_class().map_err(EngineError::WorkerW)?;
        let parent = spawn_worker_w().map_err(EngineError::WorkerW)?;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster_adapter(&instance)?;
        let (device, queue) = pollster_device(&adapter)?;
        let mut engine = Self {
            instance,
            device,
            queue,
            adapter,
            renderer: None,
            slots: Vec::new(),
            parent,
            start: Instant::now(),
            thunder_flash: 0.0,
            thunder_seed: 0.0,
        };
        engine.rebuild()?;
        Ok(engine)
    }

    pub fn rebuild(&mut self) -> Result<(), EngineError> {
        for slot in self.slots.drain(..) {
            destroy_hwnd(slot.hwnd);
        }
        let monitors = list_monitors_relative_to(self.parent);
        for monitor in monitors {
            let hwnd =
                create_monitor_window(self.parent, &monitor).map_err(EngineError::WorkerW)?;
            let target = WallpaperHwnd { hwnd };
            let unsafe_target = unsafe { SurfaceTargetUnsafe::from_window(&target) }
                .map_err(|err| EngineError::Surface(format!("{err:?}")))?;
            let surface = unsafe { self.instance.create_surface_unsafe(unsafe_target) }
                .map_err(|err| EngineError::Surface(err.to_string()))?;
            let caps = surface.get_capabilities(&self.adapter);
            let format = pick_srgb_format(&caps.formats);
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                color_space: wgpu::SurfaceColorSpace::Srgb,
                width: monitor.width.max(1),
                height: monitor.height.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: caps.alpha_modes[0],
                view_formats: vec![],
            };
            surface.configure(&self.device, &config);
            if self.renderer.is_none() {
                self.renderer = Some(SkyRenderer::new(&self.device, format));
            }
            self.slots.push(SurfaceSlot {
                hwnd,
                width: config.width,
                height: config.height,
                surface,
                config,
            });
        }
        if self.slots.is_empty() {
            return Err(EngineError::NoMonitor);
        }
        if let Some(renderer) = self.renderer.as_mut() {
            let sizes: Vec<(u32, u32)> = self
                .slots
                .iter()
                .map(|slot| (slot.width, slot.height))
                .collect();
            renderer.retain_sizes(&sizes);
        }
        Ok(())
    }

    pub fn elapsed(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }

    pub fn render(&mut self, view: &SkyView) -> Result<(), EngineError> {
        let time = self.elapsed();
        let renderer = self.renderer.as_mut().ok_or(EngineError::NoMonitor)?;
        let sizes: Vec<(u32, u32)> = self
            .slots
            .iter()
            .map(|slot| (slot.width, slot.height))
            .collect();
        renderer.retain_sizes(&sizes);
        for slot in &self.slots {
            let uniforms = SkyUniforms::from_flash(
                view,
                slot.width,
                slot.height,
                time,
                self.thunder_flash,
                self.thunder_seed,
            );
            renderer.write_uniforms(&self.queue, &uniforms);
            let frame = match slot.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    slot.surface.configure(&self.device, &slot.config);
                    continue;
                }
                wgpu::CurrentSurfaceTexture::Timeout
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Validation => continue,
            };
            let view_tex = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("sky wallpaper"),
                });
            renderer.draw(
                &self.device,
                &mut encoder,
                &view_tex,
                slot.width,
                slot.height,
            );
            self.queue.submit(Some(encoder.finish()));
            self.queue.present(frame);
        }
        Ok(())
    }

    pub fn teardown(&mut self) {
        for slot in self.slots.drain(..) {
            destroy_hwnd(slot.hwnd);
        }
    }
}

impl Drop for WallpaperEngine {
    fn drop(&mut self) {
        self.teardown();
    }
}

fn pollster_adapter(instance: &wgpu::Instance) -> Result<wgpu::Adapter, EngineError> {
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|err| EngineError::Gpu(err.to_string()))
}

fn pollster_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), EngineError> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("skywallpaper"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .map_err(|err| EngineError::Gpu(err.to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    WorkerW(#[from] WorkerWError),
    #[error("gpu: {0}")]
    Gpu(String),
    #[error("surface: {0}")]
    Surface(String),
    #[error("no monitor")]
    NoMonitor,
}
