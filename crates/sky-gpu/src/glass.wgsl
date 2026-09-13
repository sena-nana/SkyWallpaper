// Original MIT WGSL; not a CC-BY-NC-SA Shadertoy port.

struct Uniforms {
    sun_dir: vec3<f32>,
    time: f32,
    resolution: vec2<f32>,
    cloud_cover: f32,
    precip: f32,
    precip_kind: f32,
    fog: f32,
    thunder: f32,
    season: f32,
    thunder_seed: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var sky_tex: texture_2d<f32>;
@group(0) @binding(2) var sky_samp: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(p[vid], 0.0, 1.0);
}

fn slide_y(phase: f32) -> f32 {
    let p = clamp(phase, 0.0, 1.0);
    let hold = 0.38;
    let f = clamp((p - hold) / (1.0 - hold), 0.0, 1.0);
    let fall = f * f * (1.15 - 0.15 * f);
    return mix(0.05 * (p / hold), mix(0.05, 1.0, fall), step(hold, p));
}

fn lens_h(d: f32, rad: f32) -> f32 {
    let x = clamp(d / max(rad, 1e-4), 0.0, 1.0);
    let b = 1.0 - x * x;
    return b * b;
}

fn static_drops(st: vec2<f32>, t: f32, amount: f32) -> f32 {
    if (amount <= 0.0) {
        return 0.0;
    }
    let p = st * 24.0;
    let origin = floor(p);
    var h = 0.0;
    let density = mix(0.18, 0.55, amount);
    for (var jy = -1; jy <= 1; jy = jy + 1) {
        for (var ix = -1; ix <= 1; ix = ix + 1) {
            let cell = origin + vec2<f32>(f32(ix), f32(jy));
            let rnd = hash22(cell);
            let vis = smoothstep(0.0, 0.10, density - rnd.x)
                * smoothstep(0.10, 0.48, 0.5 + 0.5 * sin(t * (0.30 + rnd.y * 0.52) + rnd.x * 6.28318));
            let center = cell + 0.16 + rnd.yx * 0.68;
            h = max(h, lens_h(length(p - center), mix(0.10, 0.22, rnd.x)) * vis);
        }
    }
    return h * amount;
}

fn drop_layer(uv: vec2<f32>, aspect: f32, t: f32, cols: f32, rows: f32) -> vec2<f32> {
    let gx = uv.x * cols;
    let col = floor(gx);
    let shift = hash21(vec2<f32>(col, 2.7));
    let gy = uv.y * rows + shift;
    let cell_w = aspect / max(cols, 1.0);
    let cell_h = 1.0 / max(rows, 1.0);
    var h = 0.0;
    var wet = 0.0;
    for (var jy = -1; jy <= 1; jy = jy + 1) {
        let nrow = floor(gy) + f32(jy);
        let rnd = hash22(vec2<f32>(col, nrow));
        let phase = fract(t * mix(0.16, 0.40, rnd.y) + rnd.x);
        let y = slide_y(phase);
        let fade = smoothstep(0.0, 0.05, phase) * smoothstep(1.0, 0.88, phase);
        let falling = smoothstep(0.38, 0.55, phase);
        let cx = 0.5 + (rnd.x - 0.5) * 0.38
            + sin(phase * 16.0 + rnd.y * 9.0) * mix(0.07, 0.024, falling) * falling;
        let dx = (gx - col - cx) * cell_w;
        let fy = gy - nrow;
        let mass = 1.0 + falling * mix(0.18, 0.50, rnd.x);
        let rad = mix(0.016, 0.034, rnd.y) * mass * max(fade, 1e-4);
        let drop = lens_h(length(vec2<f32>(dx * 1.12, (fy - y) * cell_h * 0.92)), rad) * fade;
        let dist_up = clamp((y - fy) / max(y, 1e-3), 0.0, 1.0);
        let along = smoothstep(y + 0.02, y - 0.05, fy) * smoothstep(-0.02, 0.10, fy);
        let trail_w = mix(0.0032, 0.0095, rnd.x) * mix(1.20, 0.22, dist_up) * mass;
        let trail = smoothstep(trail_w, trail_w * 0.14, abs(dx)) * along * falling * fade;
        h = max(h, drop);
        wet = max(wet, trail * 0.85);
    }
    return vec2<f32>(h, wet);
}

fn drops(uv: vec2<f32>, st: vec2<f32>, aspect: f32, t: f32, rain: f32) -> vec2<f32> {
    let static_amt = smoothstep(RAIN_OVERLAY_MIN, 0.40, rain);
    let trail1 = smoothstep(0.28, 0.62, rain);
    let trail2 = smoothstep(0.50, 0.88, rain);
    let speed = mix(0.35, 1.15, rain);
    let s = static_drops(st, u.time * 0.45, static_amt);
    var d1 = vec2<f32>(0.0);
    var d2 = vec2<f32>(0.0);
    if (trail1 > 0.0) {
        d1 = drop_layer(uv, aspect, t * speed, 7.0, 1.0) * trail1;
    }
    if (trail2 > 0.0) {
        d2 = drop_layer(uv + vec2<f32>(0.17, 0.31), aspect, t * speed * 1.12, 12.0, 1.85) * trail2;
    }
    let wet = max(d1.y, d2.y);
    let eaten = mix(0.08, 1.0, 1.0 - smoothstep(0.05, 0.35, wet));
    let sliding = max(d1.x, d2.x);
    return vec2<f32>(max(s * eaten, sliding), wet);
}

fn sky_sample(uv: vec2<f32>) -> vec3<f32> {
    let p = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(sky_tex, sky_samp, p).rgb;
}

@fragment
fn fs_main(@builtin(position) clip: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = clip.xy / max(u.resolution, vec2<f32>(1.0));
    let rain = clamp(u.precip * (1.0 - u.precip_kind), 0.0, 1.0);
    if (rain <= RAIN_OVERLAY_MIN) {
        return textureSample(sky_tex, sky_samp, uv);
    }

    let aspect = u.resolution.x / max(u.resolution.y, 1.0);
    let st = vec2<f32>(uv.x * aspect, uv.y);
    let field = drops(uv, st, aspect, u.time, rain);
    let h = field.x + field.y * 0.10;
    let n = vec2<f32>(dpdx(h), dpdy(h)) * 1.35;

    let uv_r = uv + n * (0.14 * smoothstep(0.04, 0.48, rain));
    let sharp = sky_sample(uv_r);
    let texel = 6.0 / max(u.resolution, vec2<f32>(1.0));
    let blur = sky_sample(uv) * 0.36
        + sky_sample(uv + vec2<f32>(texel.x, 0.0)) * 0.16
        + sky_sample(uv - vec2<f32>(texel.x, 0.0)) * 0.16
        + sky_sample(uv + vec2<f32>(0.0, texel.y)) * 0.16
        + sky_sample(uv - vec2<f32>(0.0, texel.y)) * 0.16;
    let lum = dot(blur, vec3<f32>(0.2126, 0.7152, 0.0722));
    let muted = mix(blur, vec3<f32>(lum), 0.20) * vec3<f32>(0.84, 0.87, 0.93);
    let wipe = max(field.y, field.x * 0.25);
    let haze = mix(0.22, 0.85, rain) * (1.0 - smoothstep(0.03, 0.24, wipe));
    var col = mix(sharp, muted, haze);

    let spec_n = normalize(vec3<f32>(n * 1.8, 0.62));
    col += pow(max(dot(spec_n, normalize(vec3<f32>(-0.22, -0.72, 0.58))), 0.0), 24.0)
        * field.x
        * 0.20
        * smoothstep(0.02, 0.12, length(n));

    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
