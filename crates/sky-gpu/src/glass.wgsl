// Heartfelt - by Martijn Steinrucken aka BigWings - 2017.
// WGSL port of the original Heartfelt rain functions (CC BY-NC-SA 3.0).
// Source: https://www.shadertoy.com/view/4dS3Wd

struct Uniforms {
    sun_dir: vec3<f32>, time: f32, resolution: vec2<f32>, cloud_cover: f32,
    precip: f32, precip_kind: f32, fog: f32, thunder: f32, season: f32,
    thunder_seed: f32, _pad0: f32, _pad1: f32, _pad2: f32,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var sky_tex: texture_2d<f32>;
@group(0) @binding(2) var sky_samp: sampler;

const FAST_NORMAL_PIXEL_THRESHOLD: f32 = 3000000.0;

fn s(a: f32, b: f32, t: f32) -> f32 {
    if (a <= b) {
        return smoothstep(a, b, t);
    }
    return 1.0 - smoothstep(b, a, t);
}
fn n13(p: f32) -> vec3<f32> {
    var p3 = fract(p * vec3<f32>(0.1031, 0.11369, 0.13787));
    p3 += dot(p3, p3.yzx + vec3<f32>(19.19));
    return fract(vec3<f32>((p3.x + p3.y) * p3.z, (p3.x + p3.z) * p3.y, (p3.y + p3.z) * p3.x));
}
fn n(t: f32) -> f32 { return fract(sin(t * 12345.564) * 7658.76); }
fn saw(b: f32, t: f32) -> f32 { return s(0.0, b, t) * s(1.0, b, t); }

fn drop_layer2(uv0: vec2<f32>, t: f32) -> vec2<f32> {
    var uv = uv0; uv.y += t * 0.75;
    let a = vec2<f32>(6.0, 1.0); let grid = a * 2.0;
    var id = floor(uv * grid); uv.y += n(id.x); id = floor(uv * grid);
    let rnd = n13(id.x * 35.2 + id.y * 2376.1);
    let st = fract(uv * grid) - vec2<f32>(0.5, 0.0);
    var x = rnd.x - 0.5; let y_uv = uv0.y * 20.0;
    x += sin(y_uv + sin(y_uv)) * (0.5 - abs(x)) * (rnd.z - 0.5); x *= 0.7;
    let ti = fract(t + rnd.z); var y = (saw(0.85, ti) - 0.5) * 0.9 + 0.5;
    let d = length((st - vec2<f32>(x, y)) * a.yx); let main_drop = s(0.4, 0.0, d);
    let r = sqrt(s(1.0, y, st.y)); let cd = abs(st.x - x);
    var trail = s(0.23 * r, 0.15 * r * r, cd); let trail_front = s(-0.02, 0.02, st.y - y);
    trail *= trail_front * r * r;
    let trail2 = s(0.2 * r, 0.0, cd); y = uv0.y;
    var droplets = max(0.0, sin(y * (1.0 - y) * 120.0) - st.y) * trail2 * trail_front * rnd.z;
    y = fract(y * 10.0) + (st.y - 0.5); droplets = s(0.3, 0.0, length(st - vec2<f32>(x, y)));
    return vec2<f32>(main_drop + droplets * r * trail_front, trail);
}

fn static_drops(uv0: vec2<f32>, t: f32) -> f32 {
    let uv = uv0 * 40.0; let id = floor(uv); let st = fract(uv) - vec2<f32>(0.5);
    let rnd = n13(id.x * 107.45 + id.y * 3543.654); let p = (rnd.xy - vec2<f32>(0.5)) * 0.7;
    return s(0.3, 0.0, length(st - p)) * fract(rnd.z * 10.0) * saw(0.025, fract(t + rnd.z));
}
fn drops(uv: vec2<f32>, t: f32, l0: f32, l1: f32, l2: f32) -> vec2<f32> {
    let static_layer = static_drops(uv, t) * l0; let layer1 = drop_layer2(uv, t) * l1;
    let layer2 = drop_layer2(uv * 1.85, t) * l2; let c = static_layer + layer1.x + layer2.x;
    return vec2<f32>(s(0.3, 1.0, c), max(layer1.y * l0, layer2.y * l1));
}

fn heartfelt_rain(uv: vec2<f32>, t: f32, rain_amount: f32) -> vec3<f32> {
    let max_blur = mix(3.0, 6.0, rain_amount);
    let min_blur = 2.0;
    let static_layer = s(-0.5, 1.0, rain_amount) * 2.0;
    let layer1 = s(0.25, 0.75, rain_amount);
    let layer2 = s(0.0, 0.5, rain_amount);
    let c = drops(uv, t, static_layer, layer1, layer2);
    var normal: vec2<f32>;
    if (u.resolution.x * u.resolution.y >= FAST_NORMAL_PIXEL_THRESHOLD) {
        // At high pixel counts, reuse the neighboring fragment values already
        // present in the quad. Scale them to Heartfelt's 0.001 UV probe so the
        // refraction strength stays independent of output resolution.
        let probe_scale = 0.001 * u.resolution.y;
        normal = vec2<f32>(dpdx(c.x), -dpdy(c.x)) * probe_scale;
    } else {
        // Preserve Heartfelt's fixed probes at lower resolutions, where their
        // two extra rain evaluations are inexpensive.
        let e = vec2<f32>(0.001, 0.0);
        let cx = drops(uv + e, t, static_layer, layer1, layer2).x;
        let cy = drops(uv + e.yx, t, static_layer, layer1, layer2).x;
        normal = vec2<f32>(cx - c.x, cy - c.x);
    }
    let focus = mix(max_blur - c.y, min_blur, s(0.1, 0.2, c.x));
    return vec3<f32>(normal, focus);
}

fn sky_texel(uv: vec2<f32>) -> vec3<f32> {
    return textureSample(sky_tex, sky_samp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
}

@vertex fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(p[vid], 0.0, 1.0);
}
@fragment fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let resolution = max(u.resolution, vec2<f32>(1.0));
    let uv_screen = frag.xy / resolution;
    let rain_amount = clamp(u.precip * (1.0 - u.precip_kind), 0.0, 1.0);
    if (rain_amount <= 0.01) { return vec4<f32>(sky_texel(uv_screen), 1.0); }
    let t = u.time * 0.2;
    // Heartfelt's coordinate system is y-up; fragment coordinates are y-down.
    // Keep the background in screen space, but run the rain simulation in the
    // same orientation as the original shader so drops travel downward.
    let heartfelt_frag = vec2<f32>(frag.x, resolution.y - frag.y);
    let uv = (heartfelt_frag - resolution * 0.5) / resolution.y * 0.7;
    let background_uv = (uv_screen - vec2<f32>(0.5)) * 0.9 + vec2<f32>(0.5);
    let rain = heartfelt_rain(uv, t, rain_amount);
    let normal = rain.xy;
    let focus = rain.z;
    let warped = clamp(
        // The sky render target and the glass pass share top-left texture UVs.
        // Only the simulated y-up normal needs its sign flipped here.
        vec2<f32>(background_uv.x + normal.x, background_uv.y - normal.y),
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );
    let col = textureSampleLevel(sky_tex, sky_samp, warped, focus).rgb;
    return vec4<f32>(col, 1.0);
}
