//! Sky GPU pass.
//!
//! Display is a Helios-style OKLab mesh (off-center ellipses from the 6×4
//! palette). Sunset channel bias follows Andrew Helmer's *Production Sky
//! Rendering* (MIT, https://www.shadertoy.com/view/slSXRW) as used in
//! [dnlzro/horizon](https://github.com/dnlzro/horizon).

use sky_core::{PrecipKind, SkyView, sun_dir_2d};

const UNIFORM_SIZE: usize = 64;
/// Overlay handoff; injected into both WGSL modules.
const RAIN_OVERLAY_MIN: f32 = 0.02;

fn wgsl_source(body: &str) -> String {
    format!(
        "const RAIN_OVERLAY_MIN: f32 = {RAIN_OVERLAY_MIN:.4};\n{}\n{body}",
        include_str!("common.wgsl"),
    )
}

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
    pub thunder_seed: f32,
    pub _pad: [f32; 3],
}

impl SkyUniforms {
    pub fn from_view(
        view: &SkyView,
        width: u32,
        height: u32,
        time: f32,
        thunder_flash: f32,
    ) -> Self {
        Self::from_flash(view, width, height, time, thunder_flash, 0.0)
    }

    pub fn from_flash(
        view: &SkyView,
        width: u32,
        height: u32,
        time: f32,
        thunder_flash: f32,
        thunder_seed: f32,
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
            thunder_seed,
            _pad: [0.0; 3],
        }
    }
}

const _: () = assert!(std::mem::size_of::<SkyUniforms>() == UNIFORM_SIZE);

struct SkyTarget {
    width: u32,
    height: u32,
    _texture: wgpu::Texture,
    sky_view: wgpu::TextureView,
    glass_bg: wgpu::BindGroup,
    mip_views: Vec<wgpu::TextureView>,
    mip_bind_groups: Vec<wgpu::BindGroup>,
}

pub struct SkyRenderer {
    sky_pipeline: wgpu::RenderPipeline,
    glass_pipeline: wgpu::RenderPipeline,
    mip_pipeline: wgpu::RenderPipeline,
    sky_bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    sampler: wgpu::Sampler,
    glass_bgl: wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    targets: Vec<SkyTarget>,
    last_rain: f32,
    rain_visual: f32,
    last_time: f32,
    last_time_valid: bool,
}

fn offscreen_extent(width: u32, height: u32) -> (u32, u32) {
    (width.max(1).div_ceil(4), height.max(1).div_ceil(4))
}

fn heartfelt_mip_count(width: u32, height: u32) -> u32 {
    let full_mip_count = 32 - width.max(height).max(1).leading_zeros();
    full_mip_count.min(7)
}

fn heartfelt_focus_mip(rain: f32) -> u32 {
    (3.0 + 3.0 * rain.clamp(0.0, 1.0)).ceil() as u32
}

fn tex_view(texture: &wgpu::Texture, usage: wgpu::TextureUsages, label: &str) -> wgpu::TextureView {
    tex_mip_view(texture, usage, label, 0, Some(1))
}

fn tex_mip_view(
    texture: &wgpu::Texture,
    usage: wgpu::TextureUsages,
    label: &str,
    base_mip_level: u32,
    mip_level_count: Option<u32>,
) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        format: None,
        dimension: Some(wgpu::TextureViewDimension::D2),
        usage: Some(usage),
        aspect: wgpu::TextureAspect::All,
        base_mip_level,
        mip_level_count,
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
            source: wgpu::ShaderSource::Wgsl(wgsl_source(include_str!("sky.wgsl")).into()),
        });
        let glass_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass"),
            source: wgpu::ShaderSource::Wgsl(wgsl_source(include_str!("glass.wgsl")).into()),
        });
        let mip_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky mip"),
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
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
        let sky_pipeline =
            fullscreen_pipeline(device, "sky pipeline", &sky_layout, &sky_shader, format);
        let glass_pipeline = fullscreen_pipeline(
            device,
            "glass pipeline",
            &glass_layout,
            &glass_shader,
            format,
        );
        let mip_pipeline = fullscreen_pipeline(
            device,
            "sky mip pipeline",
            &glass_layout,
            &mip_shader,
            format,
        );
        Self {
            sky_pipeline,
            glass_pipeline,
            mip_pipeline,
            sky_bind_group,
            uniform_buf,
            sampler,
            glass_bgl,
            format,
            targets: Vec::new(),
            last_rain: 0.0,
            rain_visual: 0.0,
            last_time: 0.0,
            last_time_valid: false,
        }
    }

    pub fn write_uniforms(&mut self, queue: &wgpu::Queue, uniforms: &SkyUniforms) {
        let target = (uniforms.precip * (1.0 - uniforms.precip_kind)).clamp(0.0, 1.0);
        if uniforms.precip_kind >= 0.5 {
            // Snow and other non-rain precipitation must never take the glass
            // path while a previous rain value is fading out.
            self.rain_visual = 0.0;
            self.last_rain = 0.0;
        } else {
            let raw_dt = uniforms.time - self.last_time;
            if !self.last_time_valid {
                self.rain_visual = target;
            } else if raw_dt < -1e-5 {
                // A clock reset or resume can move time backwards. Keep the
                // current visual value for this frame instead of snapping to
                // the new target, then resume smoothing from the new epoch.
            } else if raw_dt <= 1e-5 {
                self.rain_visual = target;
            } else {
                let dt = raw_dt.min(0.1);
                let k = 1.0 - (-dt * 4.0).exp();
                self.rain_visual += (target - self.rain_visual) * k;
            }
            self.last_rain = self.rain_visual;
        }
        self.last_time_valid = true;
        self.last_time = uniforms.time;
        let mut smoothed = *uniforms;
        if uniforms.precip_kind < 0.5 {
            smoothed.precip = self.rain_visual;
        }
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&smoothed));
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
        let (ow, oh) = offscreen_extent(width, height);
        // Heartfelt's focus stays in the 2..6 range, so mip 0..6 is sufficient.
        // Avoid generating tail mips that can never be sampled by the glass pass.
        let mip_level_count = heartfelt_mip_count(ow, oh);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky offscreen"),
            size: wgpu::Extent3d {
                width: ow,
                height: oh,
                depth_or_array_layers: 1,
            },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let sky_view = tex_view(
            &texture,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            "sky offscreen rt",
        );
        let sample_view = tex_mip_view(
            &texture,
            wgpu::TextureUsages::TEXTURE_BINDING,
            "sky offscreen sample",
            0,
            Some(mip_level_count),
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
                    resource: wgpu::BindingResource::TextureView(&sample_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut mip_views = Vec::new();
        let mut mip_bind_groups = Vec::new();
        for level in 0..mip_level_count.saturating_sub(1) {
            let source_view = tex_mip_view(
                &texture,
                wgpu::TextureUsages::TEXTURE_BINDING,
                "sky mip source",
                level,
                Some(1),
            );
            mip_views.push(tex_mip_view(
                &texture,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                "sky mip target",
                level + 1,
                Some(1),
            ));
            mip_bind_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky mip bg"),
                layout: &self.glass_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            }));
        }
        SkyTarget {
            width,
            height,
            _texture: texture,
            sky_view,
            glass_bg,
            mip_views,
            mip_bind_groups,
        }
    }

    fn generate_mips(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &SkyTarget,
        max_level: u32,
    ) {
        for (level, (view, bind_group)) in target
            .mip_views
            .iter()
            .zip(&target.mip_bind_groups)
            .enumerate()
        {
            if level as u32 + 1 > max_level {
                break;
            }
            color_pass(
                encoder,
                "sky mip pass",
                view,
                &self.mip_pipeline,
                bind_group,
            );
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
        if self.last_rain <= RAIN_OVERLAY_MIN {
            self.draw_native(encoder, view);
            return;
        }
        let idx = self.ensure_target(device, width, height);
        color_pass(
            encoder,
            "sky pass",
            &self.targets[idx].sky_view,
            &self.sky_pipeline,
            &self.sky_bind_group,
        );
        self.generate_mips(
            encoder,
            &self.targets[idx],
            heartfelt_focus_mip(self.last_rain),
        );
        color_pass(
            encoder,
            "glass pass",
            view,
            &self.glass_pipeline,
            &self.targets[idx].glass_bg,
        );
    }

    fn draw_native(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        self.targets.clear();
        color_pass(
            encoder,
            "sky pass",
            view,
            &self.sky_pipeline,
            &self.sky_bind_group,
        );
    }

    #[cfg(test)]
    fn target_count(&self) -> usize {
        self.targets.len()
    }

    #[cfg(test)]
    fn draw_sky_only(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        self.draw_native(encoder, view);
    }

    #[cfg(test)]
    fn draw_sky_offscreen(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        width: u32,
        height: u32,
    ) {
        let idx = self.ensure_target(device, width, height);
        color_pass(
            encoder,
            "sky pass",
            &self.targets[idx].sky_view,
            &self.sky_pipeline,
            &self.sky_bind_group,
        );
        self.generate_mips(
            encoder,
            &self.targets[idx],
            heartfelt_focus_mip(self.last_rain),
        );
    }

    #[cfg(test)]
    fn draw_glass(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let target = self
            .targets
            .last()
            .expect("draw_sky_offscreen before draw_glass");
        color_pass(
            encoder,
            "glass pass",
            view,
            &self.glass_pipeline,
            &target.glass_bg,
        );
    }
}

pub fn wallpaper_device_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits())
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
        naga::front::wgsl::parse_str(&wgsl_source(include_str!("sky.wgsl"))).expect("sky.wgsl");
        naga::front::wgsl::parse_str(&wgsl_source(include_str!("glass.wgsl"))).expect("glass.wgsl");
        naga::front::wgsl::parse_str(include_str!("mip.wgsl")).expect("mip.wgsl");
    }

    #[test]
    fn offscreen_is_quarter() {
        assert_eq!(offscreen_extent(1920, 1080), (480, 270));
        assert_eq!(offscreen_extent(160, 90), (40, 23));
        assert_eq!(offscreen_extent(1, 1), (1, 1));
        assert_eq!(heartfelt_mip_count(480, 270), 7);
        assert_eq!(heartfelt_mip_count(40, 23), 6);
        assert_eq!(heartfelt_mip_count(1, 1), 1);
        assert_eq!(heartfelt_focus_mip(0.0), 3);
        assert_eq!(heartfelt_focus_mip(0.22), 4);
        assert_eq!(heartfelt_focus_mip(0.45), 5);
        assert_eq!(heartfelt_focus_mip(0.85), 6);
    }

    #[test]
    fn night_is_readable_indigo_and_noon_stays_brighter() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let night = sample(&device, &queue, -30.0, 0.5, SkyWeather::clear_fallback());
        let sunset = sample(&device, &queue, 0.0, 0.5, SkyWeather::clear_fallback());
        let sunset_winter = sample(&device, &queue, 0.0, 0.0, SkyWeather::clear_fallback());
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
            "sunset well should be warm {:?}",
            sunset.horizon
        );
        assert!(
            noon_z > night_z + 0.15,
            "noon should be brighter: noon={noon_z:.3} night={night_z:.3}"
        );
        let summer_warm_l = warm(sunset.left);
        let summer_warm_r = warm(sunset.right);
        assert!(
            summer_warm_l > summer_warm_r + 0.015,
            "summer dusk well should sit left: L={summer_warm_l:.3} R={summer_warm_r:.3} l={:?} r={:?}",
            sunset.left,
            sunset.right
        );
        let winter_warm_l = warm(sunset_winter.left);
        let winter_warm_r = warm(sunset_winter.right);
        assert!(
            winter_warm_r > winter_warm_l + 0.015,
            "winter dusk well should sit right: L={winter_warm_l:.3} R={winter_warm_r:.3} l={:?} r={:?}",
            sunset_winter.left,
            sunset_winter.right
        );
        let clear = SkyWeather::clear_fallback();
        sides_close(
            &sample(&device, &queue, 0.0, 0.24, clear),
            &sample(&device, &queue, 0.0, 0.26, clear),
            "equinox",
        );
        // Peak Y-wobble phase so the year wrap is not sampled at a zero crossing.
        let wrap_t = (std::f32::consts::PI - 1.7) / 0.009;
        sides_close(
            &sample_at(&device, &queue, 0.0, 0.99, clear, wrap_t),
            &sample_at(&device, &queue, 0.0, 0.01, clear, wrap_t),
            "year wrap",
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
        assert!(
            (luma(sunset.left) - luma(sunset.right)).abs() > 0.02
                || (warm(sunset.left) - warm(sunset.right)).abs() > 0.02,
            "dusk mesh should vary horizontally l={:?} r={:?}",
            sunset.left,
            sunset.right
        );

        let noon_clear = sample(&device, &queue, 50.0, 0.5, SkyWeather::clear_fallback());
        let noon_overcast = sample(
            &device,
            &queue,
            50.0,
            0.5,
            SkyWeather {
                code: WeatherCode(3),
                cloud_cover: 0.95,
                precip: 0.0,
                precip_kind: PrecipKind::Rain,
                fog: 0.0,
                thunder: false,
            },
        );
        let clear_z = luma(noon_clear.zenith);
        let over_z = luma(noon_overcast.zenith);
        assert!(
            over_z < clear_z - 0.04,
            "overcast noon zenith should be darker: over={over_z:.3} clear={clear_z:.3}"
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
    fn stars_appear_at_night_and_vanish_by_day() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let clear = SkyWeather::clear_fallback();
        let overcast = SkyWeather {
            code: WeatherCode(3),
            cloud_cover: 0.9,
            precip: 0.0,
            precip_kind: PrecipKind::Rain,
            fog: 0.0,
            thunder: false,
        };
        const W: u32 = 160;
        const H: u32 = 90;
        let night = pixels(
            &device,
            &queue,
            -30.0,
            0.5,
            clear,
            12.0,
            W,
            H,
            Frame::SkyOnly,
        );
        let twilight = pixels(
            &device,
            &queue,
            -6.0,
            0.5,
            clear,
            12.0,
            W,
            H,
            Frame::SkyOnly,
        );
        let noon = pixels(
            &device,
            &queue,
            70.0,
            0.5,
            clear,
            12.0,
            W,
            H,
            Frame::SkyOnly,
        );
        let cloudy = pixels(
            &device,
            &queue,
            -30.0,
            0.5,
            overcast,
            12.0,
            W,
            H,
            Frame::SkyOnly,
        );
        let night_peaks = isolated_peaks(&night, W, H, 0.12, 0.34);
        let twilight_peaks = isolated_peaks(&twilight, W, H, 0.12, 0.34);
        let noon_peaks = isolated_peaks(&noon, W, H, 0.12, 0.34);
        let cloudy_peaks = isolated_peaks(&cloudy, W, H, 0.12, 0.34);
        assert!(
            night_peaks >= 8,
            "clear night should show scattered star cores, got {night_peaks}"
        );
        assert!(
            noon_peaks <= 1,
            "noon should not have star cores, got {noon_peaks}"
        );
        assert!(
            twilight_peaks < night_peaks,
            "twilight stars should be weaker: twilight={twilight_peaks} night={night_peaks}"
        );
        assert!(
            cloudy_peaks < night_peaks,
            "clouds should mute stars: cloudy={cloudy_peaks} night={night_peaks}"
        );
    }

    #[test]
    fn stars_have_background_depth_without_speckle_noise() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let clear = SkyWeather::clear_fallback();
        const W: u32 = 160;
        const H: u32 = 90;
        let night = pixels(
            &device,
            &queue,
            -30.0,
            0.5,
            clear,
            12.0,
            W,
            H,
            Frame::SkyOnly,
        );
        let peaks = isolated_peaks(&night, W, H, 0.12, 0.34);
        let mid = count_luma_above(&night, W, H, 0.18, 0.72, 0.24);
        let bright = count_luma_above(&night, W, H, 0.18, 0.72, 0.52);
        assert!(
            peaks >= 5,
            "layered night should keep identifiable stars, got {peaks}"
        );
        assert!(peaks <= 90, "main stars should remain sparse, got {peaks}");
        assert!(
            mid > bright * 3,
            "background should dominate bright cores: mid={mid} bright={bright}"
        );
        assert!(
            sharp_edge_frac(&night, W, H, 0.20) < 0.10,
            "star field should use soft material edges rather than hard speckles"
        );
    }

    #[test]
    fn stars_drift_slowly_and_keep_a_continuous_band() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let clear = SkyWeather::clear_fallback();
        const W: u32 = 160;
        const H: u32 = 90;
        let t0 = pixels(
            &device,
            &queue,
            -30.0,
            0.5,
            clear,
            12.0,
            W,
            H,
            Frame::SkyOnly,
        );
        let t_short = pixels(
            &device,
            &queue,
            -30.0,
            0.5,
            clear,
            12.5,
            W,
            H,
            Frame::SkyOnly,
        );
        let t_long = pixels(
            &device,
            &queue,
            -30.0,
            0.5,
            clear,
            900.0,
            W,
            H,
            Frame::SkyOnly,
        );
        let peaks_0 = isolated_peak_positions(&t0, W, H, 0.12, 0.34);
        let peaks_short = isolated_peak_positions(&t_short, W, H, 0.12, 0.34);
        let peaks_long = isolated_peak_positions(&t_long, W, H, 0.12, 0.34);
        let short_overlap = peak_overlap(&peaks_0, &peaks_short, 2);
        let long_overlap = peak_overlap(&peaks_0, &peaks_long, 2);
        assert!(
            short_overlap > 0.55,
            "short drift should preserve star positions: overlap={short_overlap:.2}"
        );
        assert!(
            long_overlap < short_overlap * 0.85,
            "long drift should move the star positions: short={short_overlap:.2} long={long_overlap:.2}"
        );

        let upper = region_luma(&t0, W, H, 0.14, 0.34);
        let ribbon = band_luma(&t0, W, H);
        let lower = region_luma(&t0, W, H, 0.66, 0.78);
        assert!(
            (ribbon - upper).abs() > 0.004 || (ribbon - lower).abs() > 0.004,
            "star dust should form a broad composition band: upper={upper:.4} ribbon={ribbon:.4} lower={lower:.4}"
        );
    }

    #[test]
    fn thunder_lights_cloud_gaps_not_a_bolt() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let storm = thunder_wx(0.95, 0.0);
        let broken = thunder_wx(0.48, 0.0);
        let rain = thunder_wx(0.95, 0.5);
        const W: u32 = 160;
        const H: u32 = 90;
        let grab = |wx: SkyWeather, thunder: f32, frame: Frame| {
            pixels_at(&device, &queue, -18.0, 0.5, wx, 8.4, W, H, frame, thunder)
        };
        let storm_off = grab(storm, 0.0, Frame::SkyOnly);
        let storm_on = grab(storm, 1.0, Frame::SkyOnly);
        let broken_off = grab(broken, 0.0, Frame::SkyOnly);
        let broken_on = grab(broken, 1.0, Frame::SkyOnly);
        let storm_d = band_luma(&storm_on, W, H) - band_luma(&storm_off, W, H);
        let broken_d = band_luma(&broken_on, W, H) - band_luma(&broken_off, W, H);
        assert!(
            storm_d > 0.03,
            "storm thunder should brighten the cloud deck, delta={storm_d:.4}"
        );
        assert!(
            storm_d > broken_d + 0.015,
            "thunder should light overcast more than a broken deck: storm={storm_d:.4} broken={broken_d:.4}"
        );
        let edges_off = sharp_edge_frac(&storm_off, W, H, 0.22);
        let edges_on = sharp_edge_frac(&storm_on, W, H, 0.22);
        assert!(
            edges_on < edges_off + 0.012,
            "sheet flash must not add a bolt-like edge: on={edges_on:.4} off={edges_off:.4}"
        );

        let rain_off = grab(rain, 0.0, Frame::Auto);
        let rain_on = grab(rain, 1.0, Frame::Auto);
        let rain_d = band_luma(&rain_on, W, H) - band_luma(&rain_off, W, H);
        assert!(
            rain_d > 0.02,
            "thunder should still brighten a raining glass frame, delta={rain_d:.4}"
        );
        let rain_edges_off = sharp_edge_frac(&rain_off, W, H, 0.22);
        let rain_edges_on = sharp_edge_frac(&rain_on, W, H, 0.22);
        assert!(
            rain_edges_on < rain_edges_off + 0.012,
            "glass thunder must not add a bolt-like edge: on={rain_edges_on:.4} off={rain_edges_off:.4}"
        );
    }

    #[test]
    fn glass_rain_tracks_precip_and_skips_snow() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let fair = rain_wx(3, 0.0, PrecipKind::Rain);
        let storm = rain_wx(65, 0.85, PrecipKind::Rain);
        let snow = rain_wx(73, 0.85, PrecipKind::Snow);
        let mist = rain_wx(51, 0.015, PrecipKind::Rain);
        let drizzle_on_storm = rain_frame(&device, &queue, storm, Frame::Glass(0.22));
        let storm_on_storm = rain_frame(&device, &queue, storm, Frame::Glass(0.85));
        let storm_blit = rain_frame(&device, &queue, storm, Frame::Glass(0.0));
        let snow_auto = rain_frame(&device, &queue, snow, Frame::Auto);
        let snow_sky = rain_frame(&device, &queue, snow, Frame::SkyOnly);
        let fair_auto = rain_frame(&device, &queue, fair, Frame::Auto);
        let fair_sky = rain_frame(&device, &queue, fair, Frame::SkyOnly);
        let mist_auto = rain_frame(&device, &queue, mist, Frame::Auto);
        let mist_sky = rain_frame(&device, &queue, mist, Frame::SkyOnly);
        let overlay = frac_changed(&storm_on_storm, &storm_blit);
        let storm_vs_drizzle = frac_changed(&storm_on_storm, &drizzle_on_storm);
        let snow_skip = mean_abs_diff(&snow_auto, &snow_sky);
        let dry_skip = mean_abs_diff(&fair_auto, &fair_sky);
        let mist_skip = mean_abs_diff(&mist_auto, &mist_sky);
        assert!(
            overlay > 0.003,
            "glass overlay should change a rain frame vs blit {overlay:.4}"
        );
        assert!(
            storm_vs_drizzle > 0.003,
            "storm glass should differ from drizzle on the same sky {storm_vs_drizzle:.4}"
        );
        assert!(
            snow_skip < 0.0005,
            "snow should skip glass overlay {snow_skip:.4}"
        );
        assert!(
            dry_skip < 0.0005,
            "dry frames should skip glass overlay {dry_skip:.4}"
        );
        assert!(
            mist_skip < 0.0005,
            "rain below overlay min should stay native-res {mist_skip:.4}"
        );
    }

    #[test]
    fn snow_flakes_change_the_frame() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let snow = SkyWeather {
            code: WeatherCode(73),
            cloud_cover: 0.0,
            precip: 0.85,
            precip_kind: PrecipKind::Snow,
            fog: 0.0,
            thunder: false,
        };
        let a = pixels(
            &device,
            &queue,
            38.0,
            0.5,
            snow,
            1.0,
            160,
            90,
            Frame::SkyOnly,
        );
        let b = pixels(
            &device,
            &queue,
            38.0,
            0.5,
            snow,
            4.5,
            160,
            90,
            Frame::SkyOnly,
        );
        let d = frac_changed(&a, &b);
        assert!(d > 0.004, "snow flakes should move between times {d:.4}");
    }

    #[test]
    fn fog_covers_horizon_more_than_zenith() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let clear = SkyWeather {
            fog: 0.0,
            ..SkyWeather::clear_fallback()
        };
        let foggy = SkyWeather {
            fog: 0.75,
            ..SkyWeather::clear_fallback()
        };
        let a = sample(&device, &queue, 38.0, 0.5, clear);
        let b = sample(&device, &queue, 38.0, 0.5, foggy);
        let dz = rgb_dist(a.zenith, b.zenith);
        let dh = rgb_dist(a.horizon, b.horizon);
        assert!(
            dh > dz + 0.01,
            "fog should hit horizon more than zenith dh={dh:.4} dz={dz:.4}"
        );
        assert!(dh > 0.02, "fog should visibly change the horizon {dh:.4}");
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

        let snow_view = SkyView {
            sun: view.sun,
            weather: SkyWeather {
                precip_kind: PrecipKind::Snow,
                ..view.weather
            },
            season: view.season,
        };
        renderer.write_uniforms(&queue, &SkyUniforms::from_view(&snow_view, W, H, 1.1, 0.0));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.draw(&device, &mut encoder, &tex, W, H);
        queue.submit(Some(encoder.finish()));
        assert_eq!(renderer.target_count(), 0);

        renderer.write_uniforms(&queue, &SkyUniforms::from_view(&view, W, H, 1.2, 0.0));

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
        let rain_before_clock_reset = renderer.rain_visual;
        renderer.write_uniforms(&queue, &SkyUniforms::from_view(&dry_view, W, H, 0.5, 0.0));
        assert_eq!(renderer.rain_visual, rain_before_clock_reset);
        for frame in 0..12 {
            let time = 1.3 + frame as f32 * 0.1;
            renderer.write_uniforms(&queue, &SkyUniforms::from_view(&dry_view, W, H, time, 0.0));
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.draw(&device, &mut encoder, &tex, W, H);
        queue.submit(Some(encoder.finish()));
        assert_eq!(renderer.target_count(), 0);
    }

    struct Sample {
        zenith: [f32; 3],
        horizon: [f32; 3],
        left: [f32; 3],
        right: [f32; 3],
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
            required_limits: wallpaper_device_limits(&adapter),
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
        sample_at(device, queue, alt_deg, season, weather, 0.0)
    }

    fn sample_at(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        alt_deg: f64,
        season: f32,
        weather: SkyWeather,
        time: f32,
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
        renderer.write_uniforms(queue, &SkyUniforms::from_view(&view, W, H, time, 0.0));
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
        let lower_l = patch_rgb(&data, padded, W, H, W / 5, H - 6);
        let lower_c = patch_rgb(&data, padded, W, H, W / 2, H - 4);
        let lower_r = patch_rgb(&data, padded, W, H, (W * 4) / 5, H - 6);
        let horizon = brighter(brighter(lower_c, lower_l), lower_r);
        let left = patch_rgb(&data, padded, W, H, W / 6, H / 2);
        let right = patch_rgb(&data, padded, W, H, (W * 5) / 6, H / 2);
        drop(data);
        buffer.unmap();
        Sample {
            zenith,
            horizon,
            left,
            right,
        }
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

    fn warm(rgb: [f32; 3]) -> f32 {
        rgb[0] - rgb[2]
    }

    fn brighter(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        if luma(a) >= luma(b) { a } else { b }
    }

    fn rgb_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs()
    }

    fn sides_close(a: &Sample, b: &Sample, what: &str) {
        assert!(
            rgb_dist(a.left, b.left) < 0.08 && rgb_dist(a.right, b.right) < 0.08,
            "{what}: pre L/R={:?}/{:?} post L/R={:?}/{:?}",
            a.left,
            a.right,
            b.left,
            b.right
        );
    }

    fn thunder_wx(cover: f32, precip: f32) -> SkyWeather {
        SkyWeather {
            code: WeatherCode(95),
            cloud_cover: cover,
            precip,
            precip_kind: PrecipKind::Rain,
            fog: 0.0,
            thunder: true,
        }
    }

    fn rain_wx(code: u8, precip: f32, kind: PrecipKind) -> SkyWeather {
        SkyWeather {
            code: WeatherCode(code),
            cloud_cover: 0.75,
            precip,
            precip_kind: kind,
            fog: 0.0,
            thunder: false,
        }
    }

    fn rain_frame(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        weather: SkyWeather,
        frame: Frame,
    ) -> Vec<u8> {
        pixels(device, queue, 38.0, 0.5, weather, 2.4, 160, 90, frame)
    }

    #[derive(Clone, Copy)]
    enum Frame {
        Auto,
        SkyOnly,
        Glass(f32),
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
        frame: Frame,
    ) -> Vec<u8> {
        pixels_at(
            device, queue, alt_deg, season, weather, time, width, height, frame, 0.0,
        )
    }

    fn pixels_at(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        alt_deg: f64,
        season: f32,
        weather: SkyWeather,
        time: f32,
        width: u32,
        height: u32,
        frame: Frame,
        thunder: f32,
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
        let mut uniforms = SkyUniforms::from_flash(&view, w, h, time, thunder, 3.0);
        renderer.write_uniforms(queue, &uniforms);
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
        match frame {
            Frame::SkyOnly => renderer.draw_sky_only(&mut encoder, &tex),
            Frame::Auto => renderer.draw(device, &mut encoder, &tex, w, h),
            Frame::Glass(p) => {
                renderer.draw_sky_offscreen(device, &mut encoder, w, h);
                queue.submit(Some(encoder.finish()));
                uniforms.precip = p;
                renderer.write_uniforms(queue, &uniforms);
                encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                renderer.draw_glass(&mut encoder, &tex);
            }
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

    fn write_bmp(path: &std::path::Path, rgba: &[u8], w: u32, h: u32) {
        let stride = (w * 3 + 3) & !3;
        let pixel_bytes = stride * h;
        let file_size = 54u32 + pixel_bytes;
        let mut b = Vec::with_capacity(file_size as usize);
        b.extend_from_slice(b"BM");
        b.extend_from_slice(&file_size.to_le_bytes());
        b.extend_from_slice(&[0u8; 4]);
        b.extend_from_slice(&54u32.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&(w as i32).to_le_bytes());
        b.extend_from_slice(&(h as i32).to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&24u16.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&pixel_bytes.to_le_bytes());
        b.extend_from_slice(&[0u8; 16]);
        let pad = vec![0u8; (stride - w * 3) as usize];
        for y in (0..h).rev() {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                b.push(rgba[i + 2]);
                b.push(rgba[i + 1]);
                b.push(rgba[i]);
            }
            b.extend_from_slice(&pad);
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, b).expect("write rain dump");
    }

    #[test]
    #[ignore]
    fn dump_precip_looks() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/rain-dump");
        let dump = |name: &str, alt: f64, weather: SkyWeather| {
            let px = pixels(
                &device,
                &queue,
                alt,
                0.5,
                weather,
                2.4,
                960,
                540,
                Frame::Auto,
            );
            write_bmp(&dir.join(name), &px, 960, 540);
        };
        dump("drizzle.bmp", 38.0, rain_wx(51, 0.22, PrecipKind::Rain));
        dump("light.bmp", 38.0, rain_wx(61, 0.45, PrecipKind::Rain));
        dump("storm.bmp", 38.0, rain_wx(65, 0.85, PrecipKind::Rain));
        dump("storm-dusk.bmp", 2.0, rain_wx(65, 0.85, PrecipKind::Rain));
        dump("snow.bmp", 38.0, rain_wx(73, 0.85, PrecipKind::Snow));
        dump(
            "fog.bmp",
            38.0,
            SkyWeather {
                code: WeatherCode(45),
                cloud_cover: 0.55,
                precip: 0.0,
                precip_kind: PrecipKind::Rain,
                fog: 0.85,
                thunder: false,
            },
        );
    }

    #[test]
    #[ignore]
    fn dump_star_looks() {
        let (device, queue) = gpu().expect("GPU adapter required for sky look tests");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/star-dump");
        let dump = |name: &str, alt: f64, weather: SkyWeather| {
            let px = pixels(
                &device,
                &queue,
                alt,
                0.5,
                weather,
                12.0,
                960,
                540,
                Frame::SkyOnly,
            );
            write_bmp(&dir.join(name), &px, 960, 540);
        };
        dump("night-clear.bmp", -30.0, SkyWeather::clear_fallback());
        dump("dusk-clear.bmp", -6.0, SkyWeather::clear_fallback());
        dump(
            "night-cloud.bmp",
            -30.0,
            SkyWeather {
                code: WeatherCode(3),
                cloud_cover: 0.72,
                precip: 0.0,
                precip_kind: PrecipKind::Rain,
                fog: 0.0,
                thunder: false,
            },
        );
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

    fn isolated_peaks(rgba: &[u8], width: u32, height: u32, margin: f32, floor: f32) -> u32 {
        isolated_peak_positions(rgba, width, height, margin, floor).len() as u32
    }

    fn isolated_peak_positions(
        rgba: &[u8],
        width: u32,
        height: u32,
        margin: f32,
        floor: f32,
    ) -> Vec<(u32, u32)> {
        let luma_at = |x: u32, y: u32| {
            let i = ((y * width + x) * 4) as usize;
            luma([
                f32::from(rgba[i]) / 255.0,
                f32::from(rgba[i + 1]) / 255.0,
                f32::from(rgba[i + 2]) / 255.0,
            ])
        };
        let y_max = (height as f32 * 0.78) as u32;
        let mut peaks = Vec::new();
        for y in 1..y_max.saturating_sub(1).max(1) {
            for x in 1..width - 1 {
                let l = luma_at(x, y);
                if l < floor {
                    continue;
                }
                let mut neigh = 0.0f32;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        neigh = neigh.max(luma_at(
                            x.saturating_add_signed(dx),
                            y.saturating_add_signed(dy),
                        ));
                    }
                }
                if l > neigh + margin {
                    peaks.push((x, y));
                }
            }
        }
        peaks
    }

    fn peak_overlap(a: &[(u32, u32)], b: &[(u32, u32)], radius: u32) -> f32 {
        if a.is_empty() {
            return 0.0;
        }
        let radius_sq = radius * radius;
        let matched = a
            .iter()
            .filter(|&&(x, y)| {
                b.iter().any(|&(bx, by)| {
                    let dx = x.abs_diff(bx);
                    let dy = y.abs_diff(by);
                    dx * dx + dy * dy <= radius_sq
                })
            })
            .count();
        matched as f32 / a.len() as f32
    }

    fn band_luma(rgba: &[u8], width: u32, height: u32) -> f32 {
        let y0 = (height as f32 * 0.18) as u32;
        let y1 = (height as f32 * 0.62) as u32;
        let mut acc = 0.0f32;
        let mut n = 0.0f32;
        for y in y0..y1.max(y0 + 1) {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                acc += luma([
                    f32::from(rgba[i]) / 255.0,
                    f32::from(rgba[i + 1]) / 255.0,
                    f32::from(rgba[i + 2]) / 255.0,
                ]);
                n += 1.0;
            }
        }
        acc / n.max(1.0)
    }

    fn region_luma(rgba: &[u8], width: u32, height: u32, y0: f32, y1: f32) -> f32 {
        let start = (height as f32 * y0) as u32;
        let end = (height as f32 * y1) as u32;
        let mut acc = 0.0f32;
        let mut n = 0.0f32;
        for y in start..end.max(start + 1).min(height) {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                acc += luma([
                    f32::from(rgba[i]) / 255.0,
                    f32::from(rgba[i + 1]) / 255.0,
                    f32::from(rgba[i + 2]) / 255.0,
                ]);
                n += 1.0;
            }
        }
        acc / n.max(1.0)
    }

    fn count_luma_above(rgba: &[u8], width: u32, height: u32, y0: f32, y1: f32, floor: f32) -> u32 {
        let start = (height as f32 * y0) as u32;
        let end = (height as f32 * y1) as u32;
        let mut n = 0u32;
        for y in start..end.max(start + 1).min(height) {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                let value = luma([
                    f32::from(rgba[i]) / 255.0,
                    f32::from(rgba[i + 1]) / 255.0,
                    f32::from(rgba[i + 2]) / 255.0,
                ]);
                if value > floor {
                    n += 1;
                }
            }
        }
        n
    }

    fn sharp_edge_frac(rgba: &[u8], width: u32, height: u32, jump: f32) -> f32 {
        let luma_at = |x: u32, y: u32| {
            let i = ((y * width + x) * 4) as usize;
            luma([
                f32::from(rgba[i]) / 255.0,
                f32::from(rgba[i + 1]) / 255.0,
                f32::from(rgba[i + 2]) / 255.0,
            ])
        };
        let mut sharp = 0.0f32;
        let mut n = 0.0f32;
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                n += 1.0;
                let l = luma_at(x, y);
                let d = (l - luma_at(x + 1, y))
                    .abs()
                    .max((l - luma_at(x - 1, y)).abs())
                    .max((l - luma_at(x, y + 1)).abs())
                    .max((l - luma_at(x, y - 1)).abs());
                if d > jump {
                    sharp += 1.0;
                }
            }
        }
        sharp / n.max(1.0)
    }
}
