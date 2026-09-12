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

struct SkyTarget {
    width: u32,
    height: u32,
    _texture: wgpu::Texture,
    mip0_view: wgpu::TextureView,
    glass_bg: wgpu::BindGroup,
    mip_src_bgs: Vec<wgpu::BindGroup>,
    mip_dst_views: Vec<wgpu::TextureView>,
}

pub struct SkyRenderer {
    sky_pipeline: wgpu::RenderPipeline,
    glass_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    sky_bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    sampler: wgpu::Sampler,
    glass_bgl: wgpu::BindGroupLayout,
    blit_bgl: wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    targets: Vec<SkyTarget>,
    last_rain: f32,
}

fn mip_count_for(width: u32, height: u32) -> u32 {
    let max_dim = width.max(height).max(1);
    (max_dim.ilog2() + 1).min(6).max(1)
}

fn mip_view(
    texture: &wgpu::Texture,
    base: u32,
    count: u32,
    usage: wgpu::TextureUsages,
    label: &str,
) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        format: None,
        dimension: Some(wgpu::TextureViewDimension::D2),
        usage: Some(usage),
        aspect: wgpu::TextureAspect::All,
        base_mip_level: base,
        mip_level_count: Some(count),
        base_array_layer: 0,
        array_layer_count: Some(1),
    })
}

fn color_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
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
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
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
    })
}

impl SkyRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let glass_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glass.wgsl").into()),
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mip blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mip.wgsl").into()),
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky uniforms"),
            size: std::mem::size_of::<SkyUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sky_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
        let glass_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mip blit bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sky_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky bg"),
            layout: &sky_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky glass sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: 32.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });
        let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky pll"),
            bind_group_layouts: &[Some(&sky_bgl)],
            immediate_size: 0,
        });
        let glass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass pll"),
            bind_group_layouts: &[Some(&glass_bgl)],
            immediate_size: 0,
        });
        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mip blit pll"),
            bind_group_layouts: &[Some(&blit_bgl)],
            immediate_size: 0,
        });
        let sky_pipeline =
            fullscreen_pipeline(device, "sky pipeline", &sky_layout, &sky_shader, format);
        let glass_pipeline = fullscreen_pipeline(
            device,
            "glass pipeline",
            &glass_layout,
            &glass_shader,
            format,
        );
        let blit_pipeline = fullscreen_pipeline(
            device,
            "mip blit pipeline",
            &blit_layout,
            &blit_shader,
            format,
        );
        Self {
            sky_pipeline,
            glass_pipeline,
            blit_pipeline,
            sky_bind_group,
            uniform_buf,
            sampler,
            glass_bgl,
            blit_bgl,
            format,
            targets: Vec::new(),
            last_rain: 0.0,
        }
    }

    pub fn write_uniforms(&mut self, queue: &wgpu::Queue, uniforms: &SkyUniforms) {
        self.last_rain = uniforms.precip * (1.0 - uniforms.precip_kind);
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(uniforms));
    }

    pub fn retain_sizes(&mut self, sizes: &[(u32, u32)]) {
        self.targets
            .retain(|t| sizes.iter().any(|&(w, h)| w == t.width && h == t.height));
    }

    fn ensure_target(&mut self, device: &wgpu::Device, width: u32, height: u32) -> usize {
        if let Some(i) = self
            .targets
            .iter()
            .position(|t| t.width == width && t.height == height)
        {
            return i;
        }
        self.targets
            .push(self.create_target(device, width.max(1), height.max(1)));
        self.targets.len() - 1
    }

    fn create_target(&self, device: &wgpu::Device, width: u32, height: u32) -> SkyTarget {
        let mip_count = mip_count_for(width, height);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mip0_view = mip_view(
            &texture,
            0,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            "sky mip0 rt",
        );
        let full_view = mip_view(
            &texture,
            0,
            mip_count,
            wgpu::TextureUsages::TEXTURE_BINDING,
            "sky mips",
        );
        let glass_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass bg"),
            layout: &self.glass_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&full_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut mip_src_bgs = Vec::new();
        let mut mip_dst_views = Vec::new();
        for i in 0..mip_count.saturating_sub(1) {
            let src = mip_view(
                &texture,
                i,
                1,
                wgpu::TextureUsages::TEXTURE_BINDING,
                "sky mip src",
            );
            mip_src_bgs.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mip blit bg"),
                layout: &self.blit_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            }));
            mip_dst_views.push(mip_view(
                &texture,
                i + 1,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                "sky mip dst",
            ));
        }
        SkyTarget {
            width,
            height,
            _texture: texture,
            mip0_view,
            glass_bg,
            mip_src_bgs,
            mip_dst_views,
        }
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        if self.last_rain <= 0.0001 {
            self.targets.clear();
            color_pass(
                encoder,
                "sky pass",
                view,
                &self.sky_pipeline,
                &self.sky_bind_group,
            );
            return;
        }
        let idx = self.ensure_target(device, width, height);
        color_pass(
            encoder,
            "sky pass",
            &self.targets[idx].mip0_view,
            &self.sky_pipeline,
            &self.sky_bind_group,
        );
        let target = &self.targets[idx];
        for i in 0..target.mip_dst_views.len() {
            color_pass(
                encoder,
                "sky mip blit",
                &target.mip_dst_views[i],
                &self.blit_pipeline,
                &target.mip_src_bgs[i],
            );
        }
        color_pass(
            encoder,
            "glass pass",
            view,
            &self.glass_pipeline,
            &target.glass_bg,
        );
    }

    #[cfg(test)]
    fn target_count(&self) -> usize {
        self.targets.len()
    }

    #[cfg(test)]
    fn draw_sky_only(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        self.targets.clear();
        color_pass(
            encoder,
            "sky pass",
            view,
            &self.sky_pipeline,
            &self.sky_bind_group,
        );
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
        naga::front::wgsl::parse_str(include_str!("glass.wgsl")).expect("glass.wgsl");
        naga::front::wgsl::parse_str(include_str!("mip.wgsl")).expect("mip.wgsl");
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

    #[test]
    fn glass_rain_tracks_precip_and_skips_snow() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let fair = SkyWeather {
            code: WeatherCode(3),
            cloud_cover: 0.75,
            precip: 0.0,
            precip_kind: PrecipKind::Rain,
            fog: 0.0,
            thunder: false,
        };
        let drizzle = SkyWeather {
            code: WeatherCode(51),
            cloud_cover: 0.75,
            precip: 0.22,
            precip_kind: PrecipKind::Rain,
            fog: 0.0,
            thunder: false,
        };
        let storm = SkyWeather {
            code: WeatherCode(65),
            cloud_cover: 0.75,
            precip: 0.85,
            precip_kind: PrecipKind::Rain,
            fog: 0.0,
            thunder: false,
        };
        let snow = SkyWeather {
            code: WeatherCode(73),
            cloud_cover: 0.75,
            precip: 0.85,
            precip_kind: PrecipKind::Snow,
            fog: 0.0,
            thunder: false,
        };
        let drizzle_glass = pixels(&device, &queue, 38.0, 0.5, drizzle, 2.4, 160, 90, false);
        let drizzle_sky = pixels(&device, &queue, 38.0, 0.5, drizzle, 2.4, 160, 90, true);
        let storm_glass = pixels(&device, &queue, 38.0, 0.5, storm, 2.4, 160, 90, false);
        let storm_sky = pixels(&device, &queue, 38.0, 0.5, storm, 2.4, 160, 90, true);
        let snow_auto = pixels(&device, &queue, 38.0, 0.5, snow, 2.4, 160, 90, false);
        let snow_sky = pixels(&device, &queue, 38.0, 0.5, snow, 2.4, 160, 90, true);
        let fair_auto = pixels(&device, &queue, 38.0, 0.5, fair, 2.4, 160, 90, false);
        let fair_sky = pixels(&device, &queue, 38.0, 0.5, fair, 2.4, 160, 90, true);
        let overlay = frac_changed(&drizzle_glass, &drizzle_sky);
        let storm_overlay = frac_changed(&storm_glass, &storm_sky);
        let snow_skip = mean_abs_diff(&snow_auto, &snow_sky);
        let dry_skip = mean_abs_diff(&fair_auto, &fair_sky);
        assert!(
            overlay > 0.01,
            "glass overlay should change a rain frame vs sky-only {overlay:.4}"
        );
        assert!(
            storm_overlay > overlay,
            "storm overlay should cover more pixels than drizzle {storm_overlay:.4} vs {overlay:.4}"
        );
        assert!(
            snow_skip < 0.0005,
            "snow should skip glass overlay {snow_skip:.4}"
        );
        assert!(
            dry_skip < 0.0005,
            "dry frames should skip glass overlay {dry_skip:.4}"
        );
    }

    #[test]
    fn retain_sizes_drops_stale_offscreen() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut renderer = SkyRenderer::new(&device, format);
        let weather = SkyWeather {
            code: WeatherCode(63),
            cloud_cover: 0.8,
            precip: 0.7,
            precip_kind: PrecipKind::Rain,
            fog: 0.0,
            thunder: false,
        };
        let view = SkyView {
            sun: SunState { altitude_deg: 30.0 },
            weather,
            season: 0.5,
        };
        const W: u32 = 64;
        const H: u32 = 48;
        renderer.write_uniforms(&queue, &SkyUniforms::from_view(&view, W, H, 1.0, 0.0));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("retain-test"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let tex = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.draw(&device, &mut encoder, &tex, W, H);
        queue.submit(Some(encoder.finish()));
        assert_eq!(renderer.target_count(), 1);
        renderer.retain_sizes(&[]);
        assert_eq!(renderer.target_count(), 0);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.draw(&device, &mut encoder, &tex, W, H);
        queue.submit(Some(encoder.finish()));
        assert_eq!(renderer.target_count(), 1);
        renderer.retain_sizes(&[(32, 24)]);
        assert_eq!(renderer.target_count(), 0);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.draw(&device, &mut encoder, &tex, W, H);
        queue.submit(Some(encoder.finish()));
        assert_eq!(renderer.target_count(), 1);
        let mut dry = view.weather;
        dry.precip = 0.0;
        let dry_view = SkyView {
            sun: view.sun,
            weather: dry,
            season: view.season,
        };
        renderer.write_uniforms(&queue, &SkyUniforms::from_view(&dry_view, W, H, 1.0, 0.0));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.draw(&device, &mut encoder, &tex, W, H);
        queue.submit(Some(encoder.finish()));
        assert_eq!(renderer.target_count(), 0);
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
        let mut renderer = SkyRenderer::new(device, format);
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
        renderer.draw(
            device,
            &mut encoder,
            &texture.create_view(&Default::default()),
            W,
            H,
        );
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

    fn pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        alt_deg: f64,
        season: f32,
        weather: SkyWeather,
        time: f32,
        width: u32,
        height: u32,
        sky_only: bool,
    ) -> Vec<u8> {
        let w = width;
        let h = height;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut renderer = SkyRenderer::new(device, format);
        let view = SkyView {
            sun: SunState {
                altitude_deg: alt_deg,
            },
            weather,
            season,
        };
        renderer.write_uniforms(queue, &SkyUniforms::from_view(&view, w, h, time, 0.0));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky-rain-test"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
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
        let tex = texture.create_view(&Default::default());
        if sky_only {
            renderer.draw_sky_only(&mut encoder, &tex);
        } else {
            renderer.draw(device, &mut encoder, &tex, w, h);
        }
        let padded = (w * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky-rain-read"),
            size: u64::from(padded * h),
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
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for rain readback");
        let data = slice.get_mapped_range().expect("map rain readback");
        let packed = (w * 4) as usize;
        let mut out = Vec::with_capacity(packed * h as usize);
        for y in 0..h {
            let row = (y * padded) as usize;
            out.extend_from_slice(&data[row..row + packed]);
        }
        drop(data);
        buffer.unmap();
        out
    }

    fn frac_changed(a: &[u8], b: &[u8]) -> f32 {
        assert_eq!(a.len(), b.len());
        let mut changed = 0.0f32;
        let mut n = 0.0f32;
        for (chunk_a, chunk_b) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
            n += 1.0;
            let dr = chunk_a[0].abs_diff(chunk_b[0]);
            let dg = chunk_a[1].abs_diff(chunk_b[1]);
            let db = chunk_a[2].abs_diff(chunk_b[2]);
            if dr.max(dg).max(db) > 3 {
                changed += 1.0;
            }
        }
        changed / n.max(1.0)
    }

    fn mean_abs_diff(a: &[u8], b: &[u8]) -> f32 {
        assert_eq!(a.len(), b.len());
        let mut acc = 0.0f32;
        let mut n = 0.0f32;
        for (&x, &y) in a.iter().zip(b.iter()) {
            acc += (f32::from(x) - f32::from(y)).abs() / 255.0;
            n += 1.0;
        }
        acc / n.max(1.0)
    }
}
