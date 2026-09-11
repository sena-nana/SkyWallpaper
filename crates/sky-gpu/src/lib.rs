//! Physical sky GPU pass.
//!
//! Atmosphere coefficients and single-scattering follow Hillaire 2020 via
//! Andrew Helmer's *Production Sky Rendering* (MIT,
//! https://www.shadertoy.com/view/slSXRW) as implemented in
//! [dnlzro/horizon](https://github.com/dnlzro/horizon) `src/gradient.ts`.

use sky_core::{PrecipKind, SkyView};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkyUniforms {
    pub sun_dir: [f32; 3],
    pub time: f32,
    pub moon_dir: [f32; 3],
    pub moon_phase: f32,
    pub resolution: [f32; 2],
    pub cloud_cover: f32,
    pub precip: f32,
    pub precip_kind: f32,
    pub fog: f32,
    pub thunder: f32,
    pub latitude: f32,
    pub sidereal: f32,
    pub cam_pitch: f32,
    pub cam_yaw: f32,
    pub exposure: f32,
}

impl SkyUniforms {
    pub fn from_view(
        view: &SkyView,
        width: u32,
        height: u32,
        time: f32,
        thunder_flash: f32,
    ) -> Self {
        Self {
            sun_dir: view.sun.dir_enu,
            time,
            moon_dir: view.moon.dir_enu,
            moon_phase: view.moon.illumination as f32,
            resolution: [width as f32, height as f32],
            cloud_cover: view.weather.cloud_cover,
            precip: view.weather.precip,
            precip_kind: match view.weather.precip_kind {
                PrecipKind::Rain => 0.0,
                PrecipKind::Snow => 1.0,
            },
            fog: view.weather.fog,
            thunder: thunder_flash.clamp(0.0, 1.0),
            latitude: view.latitude_deg as f32,
            sidereal: view.sidereal_deg as f32,
            cam_pitch: view.cam_pitch_deg,
            cam_yaw: view.cam_yaw_deg,
            exposure: view.exposure,
        }
    }
}

pub struct SkyRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
}

impl SkyRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky uniforms"),
            size: std::mem::size_of::<SkyUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky pll"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            bind_group,
            uniform_buf,
        }
    }

    pub fn write_uniforms(&self, queue: &wgpu::Queue, uniforms: &SkyUniforms) {
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(uniforms));
    }

    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sky pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

pub fn pick_srgb_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .or_else(|| formats.first().copied())
        .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb)
}
