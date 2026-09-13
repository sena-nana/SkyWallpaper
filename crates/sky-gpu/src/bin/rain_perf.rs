use std::time::Instant;

use sky_core::{PrecipKind, SkyView, SkyWeather, SunState, WeatherCode};
use sky_gpu::{SkyRenderer, SkyUniforms, wallpaper_device_limits};

const WARMUP_FRAMES: u32 = 8;
const MEASURE_FRAMES: u32 = 60;
const FRAME_STEP: f32 = 1.0 / 30.0;

const SCENARIOS: [(&str, f32); 3] = [("drizzle", 0.22), ("light", 0.45), ("storm", 0.85)];

fn percentile(values: &mut [f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * p).round() as usize;
    values[index]
}

fn weather(precip: f32) -> SkyWeather {
    SkyWeather {
        code: WeatherCode(65),
        cloud_cover: 0.85,
        precip,
        precip_kind: PrecipKind::Rain,
        fog: 0.0,
        thunder: false,
    }
}

fn output_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rain perf output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn main() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .expect("GPU adapter required for rain_perf");
    let supported = adapter.features();
    let timestamps = supported.contains(wgpu::Features::TIMESTAMP_QUERY)
        && supported.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
    let required_features = if timestamps {
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
    } else {
        wgpu::Features::empty()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rain perf device"),
        required_features,
        required_limits: wallpaper_device_limits(&adapter),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .expect("rain_perf device");

    let info = adapter.get_info();
    println!(
        "adapter={:?};backend={:?};timestamps={};timestamp_period_ns={}",
        info.name,
        info.backend,
        timestamps,
        queue.get_timestamp_period()
    );
    println!("scenario,width,height,cpu_wait_avg_ms,cpu_wait_p95_ms,gpu_avg_ms,gpu_p95_ms");

    for &(width, height) in &[(1920, 1080), (3840, 2160)] {
        for (scenario, precip) in SCENARIOS {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let mut renderer = SkyRenderer::new(&device, format);
            let texture = output_texture(&device, format, width, height);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let sky = SkyView {
                sun: SunState { altitude_deg: 38.0 },
                weather: weather(precip),
                season: 0.5,
            };
            let query_set = timestamps.then(|| {
                device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("rain perf timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: MEASURE_FRAMES * 2,
                })
            });

            let mut time = 10.0;
            for _ in 0..WARMUP_FRAMES {
                renderer.write_uniforms(
                    &queue,
                    &SkyUniforms::from_view(&sky, width, height, time, 0.0),
                );
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rain perf warmup"),
                });
                renderer.draw(&device, &mut encoder, &view, width, height);
                queue.submit(Some(encoder.finish()));
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("warmup poll");
                time += FRAME_STEP;
            }

            let mut cpu_ms = Vec::with_capacity(MEASURE_FRAMES as usize);
            for frame in 0..MEASURE_FRAMES {
                renderer.write_uniforms(
                    &queue,
                    &SkyUniforms::from_view(&sky, width, height, time, 0.0),
                );
                let started = Instant::now();
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rain perf frame"),
                });
                if let Some(ref queries) = query_set {
                    encoder.write_timestamp(queries, frame * 2);
                }
                renderer.draw(&device, &mut encoder, &view, width, height);
                if let Some(ref queries) = query_set {
                    encoder.write_timestamp(queries, frame * 2 + 1);
                }
                queue.submit(Some(encoder.finish()));
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("frame poll");
                cpu_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                time += FRAME_STEP;
            }

            let mut cpu_p95_values = cpu_ms.clone();
            let cpu_avg = cpu_ms.iter().sum::<f64>() / cpu_ms.len() as f64;
            let cpu_p95 = percentile(&mut cpu_p95_values, 0.95);
            let (gpu_avg, gpu_p95) = if let Some(queries) = query_set {
                let bytes_per_query = u64::from(wgpu::QUERY_SIZE);
                let result_size = bytes_per_query * u64::from(MEASURE_FRAMES * 2);
                let result = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("rain perf query results"),
                    size: result_size,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("rain perf query readback"),
                    size: result_size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rain perf resolve"),
                });
                encoder.resolve_query_set(&queries, 0..MEASURE_FRAMES * 2, &result, 0);
                encoder.copy_buffer_to_buffer(&result, 0, &readback, 0, result_size);
                queue.submit(Some(encoder.finish()));
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("query poll");
                let slice = readback.slice(..);
                slice.map_async(wgpu::MapMode::Read, |_| {});
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("query map");
                let data = slice.get_mapped_range().expect("query data map");
                let timestamps_ns: Vec<u64> = data
                    .chunks_exact(8)
                    .map(|bytes| u64::from_le_bytes(bytes.try_into().expect("timestamp bytes")))
                    .collect();
                let period = f64::from(queue.get_timestamp_period());
                let mut frame_gpu_ms = Vec::with_capacity(MEASURE_FRAMES as usize);
                for pair in timestamps_ns.chunks_exact(2) {
                    frame_gpu_ms
                        .push((pair[1].saturating_sub(pair[0]) as f64) * period / 1_000_000.0);
                }
                drop(data);
                readback.unmap();
                let mut p95_values = frame_gpu_ms.clone();
                (
                    frame_gpu_ms.iter().sum::<f64>() / frame_gpu_ms.len() as f64,
                    percentile(&mut p95_values, 0.95),
                )
            } else {
                (f64::NAN, f64::NAN)
            };
            println!(
                "{},{},{},{:.3},{:.3},{:.3},{:.3}",
                scenario, width, height, cpu_avg, cpu_p95, gpu_avg, gpu_p95
            );
        }
    }
}
