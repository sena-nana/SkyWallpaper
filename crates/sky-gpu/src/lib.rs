//! Physical sky GPU pass.
//!
//! Atmosphere coefficients and single-scattering follow Hillaire 2020 via
//! Andrew Helmer's *Production Sky Rendering* (MIT,
//! https://www.shadertoy.com/view/slSXRW) as implemented in
//! [dnlzro/horizon](https://github.com/dnlzro/horizon) `src/gradient.ts`.

use sky_core::{PrecipKind, SkyView, sun_dir_2d};

const UNIFORM_SIZE: usize = 48;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkyUniforms {
    pub sun_dir: [f32; 3],
    pub time: f32,
    pub resolution: [f32; 2],
    pub cloud_cover: f32,
    pub precip: f32,
    pub precip_kind: f32,
    pub fog: f32,
    pub thunder: f32,
    pub season: f32,
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
            sun_dir: sun_dir_2d(view.sun.altitude_deg),
            time,
            resolution: [width as f32, height as f32],
            cloud_cover: view.weather.cloud_cover,
            precip: view.weather.precip,
            precip_kind: match view.weather.precip_kind {
                PrecipKind::Rain => 0.0,
                PrecipKind::Snow => 1.0,
            },
            fog: view.weather.fog,
            thunder: thunder_flash.clamp(0.0, 1.0),
            season: view.season.rem_euclid(1.0),
        }
    }
}

const _: () = assert!(std::mem::size_of::<SkyUniforms>() == UNIFORM_SIZE);

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

#[cfg(test)]
mod tests {
    use super::*;
    use sky_core::{PrecipKind, SkyView, SkyWeather, SunState, WeatherCode};

    #[test]
    fn sky_shader_parses() {
        naga::front::wgsl::parse_str(include_str!("sky.wgsl")).expect("sky.wgsl");
    }

    #[test]
    fn night_is_readable_indigo_and_noon_stays_brighter() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let night = sample(&device, &queue, -30.0, 0.5, SkyWeather::clear_fallback());
        let sunset = sample(&device, &queue, 0.0, 0.5, SkyWeather::clear_fallback());
        let noon = sample(&device, &queue, 70.0, 0.5, SkyWeather::clear_fallback());
        let night_z = luma(night.zenith);
        let night_h = luma(night.horizon);
        let noon_z = luma(noon.zenith);
        assert!(
            night_z > 0.10 && night_z < 0.32,
            "night zenith luma {night_z:.3} rgba={:?}",
            night.zenith
        );
        assert!(
            night_h > night_z,
            "horizon should lift: h={night_h:.3} z={night_z:.3}"
        );
        assert!(
            night.zenith[2] > night.zenith[0] + 0.03,
            "night zenith should be blue {:?}",
            night.zenith
        );
        assert!(
            sunset.horizon[0] > sunset.horizon[2],
            "sunset horizon should be warm {:?}",
            sunset.horizon
        );
        assert!(
            noon_z > night_z + 0.15,
            "noon should be brighter: noon={noon_z:.3} night={night_z:.3}"
        );

        let winter_night = sample(
            &device,
            &queue,
            -30.0,
            0.0,
            SkyWeather {
                code: WeatherCode(73),
                cloud_cover: 0.9,
                precip: 0.8,
                precip_kind: PrecipKind::Snow,
                fog: 0.55,
                thunder: false,
            },
        );
        assert!(
            winter_night.zenith[2] > winter_night.zenith[0] + 0.02,
            "snow/fog/cloud night should keep look hue {:?}",
            winter_night.zenith
        );
        assert!(
            winter_night.horizon[2] > winter_night.horizon[0],
            "foggy night horizon should stay cool {:?}",
            winter_night.horizon
        );
        let sunset_cloud = sample(
            &device,
            &queue,
            0.0,
            0.5,
            SkyWeather {
                code: WeatherCode(2),
                cloud_cover: 0.7,
                precip: 0.0,
                precip_kind: PrecipKind::Rain,
                fog: 0.0,
                thunder: false,
            },
        );
        assert!(
            sunset_cloud.horizon[0] > sunset_cloud.horizon[2],
            "sunset clouds should stay warm {:?}",
            sunset_cloud.horizon
        );

        let winter_noon = sample(&device, &queue, 50.0, 0.0, SkyWeather::clear_fallback());
        let summer_noon = sample(&device, &queue, 50.0, 0.5, SkyWeather::clear_fallback());
        assert!(
            summer_noon.zenith[2] > summer_noon.zenith[0],
            "summer noon zenith should be cyan {:?}",
            summer_noon.zenith
        );
        let summer_cyan = summer_noon.zenith[2] - summer_noon.zenith[0];
        let winter_cyan = winter_noon.zenith[2] - winter_noon.zenith[0];
        assert!(
            summer_cyan > winter_cyan + 0.02,
            "summer noon should be more cyan than winter: summer={summer_cyan:.3} winter={winter_cyan:.3} s={:?} w={:?}",
            summer_noon.zenith,
            winter_noon.zenith
        );
    }

    struct Sample {
        zenith: [f32; 3],
        horizon: [f32; 3],
    }

    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("sky-gpu-test"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .ok()
    }

    fn sample(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        alt_deg: f64,
        season: f32,
        weather: SkyWeather,
    ) -> Sample {
        const W: u32 = 64;
        const H: u32 = 48;
        // Unorm (not sRGB) so readback is the shader's display-referred output.
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let renderer = SkyRenderer::new(device, format);
        let view = SkyView {
            sun: SunState {
                altitude_deg: alt_deg,
            },
            weather,
            season,
        };
        renderer.write_uniforms(queue, &SkyUniforms::from_view(&view, W, H, 0.0, 0.0));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky-test"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.draw(&mut encoder, &texture.create_view(&Default::default()));
        let padded = (W * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky-read"),
            size: u64::from(padded * H),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(H),
                },
            },
            wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for sky readback");
        let data = slice.get_mapped_range().expect("map sky readback");
        let zenith = patch_rgb(&data, padded, W, H, W / 2, 3);
        let horizon = patch_rgb(&data, padded, W, H, W / 2, H - 4);
        drop(data);
        buffer.unmap();
        Sample { zenith, horizon }
    }

    fn patch_rgb(data: &[u8], padded: u32, width: u32, height: u32, cx: u32, cy: u32) -> [f32; 3] {
        let mut acc = [0.0f32; 3];
        let mut n = 0.0;
        for dy in 0..3u32 {
            for dx in 0..3u32 {
                let x = (cx + dx).min(width - 1);
                let y = (cy + dy).min(height - 1);
                let i = (y * padded + x * 4) as usize;
                acc[0] += f32::from(data[i]) / 255.0;
                acc[1] += f32::from(data[i + 1]) / 255.0;
                acc[2] += f32::from(data[i + 2]) / 255.0;
                n += 1.0;
            }
        }
        [acc[0] / n, acc[1] / n, acc[2] / n]
    }

    fn luma(rgb: [f32; 3]) -> f32 {
        0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
    }
}
