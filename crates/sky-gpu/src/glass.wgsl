// Full-screen wet-glass overlay.

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

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let kk = max(k, 1e-4);
    let h = clamp(0.5 + 0.5 * (b - a) / kk, 0.0, 1.0);
    return mix(b, a, h) - kk * h * (1.0 - h);
}

fn slide_y(phase: f32) -> f32 {
    let p = clamp(phase, 0.0, 1.0);
    let hold = 0.38;
    let f = clamp((p - hold) / (1.0 - hold), 0.0, 1.0);
    let fall = f * f * (1.15 - 0.15 * f);
    return mix(0.05 * (p / hold), mix(0.05, 1.0, fall), step(hold, p));
}

fn sd_drop(q: vec2<f32>, rad: f32, fall: f32) -> f32 {
    var p = q;
    p.y -= rad * 0.22 * fall;
    let pinch = 1.0 + max(-p.y, 0.0) * mix(0.18, 1.55, fall);
    p.x *= mix(1.22, 1.42, fall) * pinch;
    p.y *= mix(0.80, 0.60, fall);
    return length(p) - rad;
}

fn height_from_sd(sd: f32, depth: f32) -> f32 {
    let x = clamp(-sd / max(depth, 1e-4), 0.0, 1.0);
    return x * x * (3.0 - 2.0 * x);
}

fn static_drops(st: vec2<f32>, t: f32, amount: f32) -> f32 {
    if (amount <= 0.0) {
        return 0.0;
    }
    let scale = 18.0;
    let p = st * scale;
    let origin = floor(p);
    var sd = 8.0;
    let density = 0.58 * amount;
    for (var jy = -1; jy <= 1; jy = jy + 1) {
        for (var ix = -1; ix <= 1; ix = ix + 1) {
            let cell = origin + vec2<f32>(f32(ix), f32(jy));
            let rnd = hash22(cell);
            let spawn = smoothstep(0.0, 0.08, density - rnd.x);
            let center = cell + vec2<f32>(0.14, 0.14) + hash22(cell + vec2<f32>(3.1, 7.7)) * 0.72;
            let life = 0.5 + 0.5 * sin(t * (0.32 + rnd.y * 0.55) + rnd.x * 6.28318);
            let vis = smoothstep(0.10, 0.52, life) * spawn;
            let rad = mix(0.060, 0.18, fract(rnd.x * 13.0)) * vis;
            let d = sd_drop(p - center, max(rad, 1e-4), 0.0) + step(vis, 0.001) * 0.5;
            sd = smin(sd, d, mix(0.001, 0.20, vis));
        }
    }
    return height_from_sd(sd, 0.14) * amount;
}

fn drop_layer(uv: vec2<f32>, aspect: f32, t: f32, cols: f32, rows: f32) -> vec2<f32> {
    let px = uv.x * cols;
    let col = floor(px);
    let stretch = vec2<f32>(aspect / max(cols, 1.0), 1.0 / max(rows, 1.0));
    let py = uv.y * rows + hash21(vec2<f32>(col, 2.7));
    var sd = 8.0;
    var trail_h = 0.0;

    for (var jy = -1; jy <= 1; jy = jy + 1) {
        let nrow = floor(py) + f32(jy);
        let fy = py - nrow;
        let id = vec2<f32>(col, nrow);
        let rnd = hash22(id);
        let phase = fract(t * mix(0.16, 0.40, rnd.y) + rnd.x);
        let y = slide_y(phase);
        let fade = smoothstep(0.0, 0.05, phase) * smoothstep(1.0, 0.88, phase);
        let falling = smoothstep(0.38, 0.55, phase);
        let wig = sin(phase * 16.0 + rnd.y * 9.0) * mix(0.08, 0.028, falling) * falling;
        let cx = 0.5 + (rnd.x - 0.5) * 0.42 + wig;
        let mass = 1.0 + smoothstep(0.38, 0.96, phase) * mix(0.28, 0.75, rnd.x);
        let rad = mix(0.012, 0.026, rnd.y) * mass * fade;
        let q = vec2<f32>((px - (col + cx)) * stretch.x, (fy - y) * stretch.y);
        sd = smin(
            sd,
            sd_drop(q, max(rad, 1e-4), falling * fade) + step(fade, 0.001) * 0.5,
            mix(0.001, 0.018, fade),
        );

        let dist_up = clamp((y - fy) / max(y, 1e-3), 0.0, 1.0);
        let along = smoothstep(y + 0.02, y - 0.06, fy) * smoothstep(-0.02, 0.10, fy);
        let trail_w = mix(0.0034, 0.010, rnd.x) * mix(1.25, 0.28, dist_up) * mass;
        let dx = abs(px - (col + cx)) * stretch.x;
        let trail = smoothstep(trail_w, trail_w * 0.16, dx) * along * falling * fade;
        trail_h = max(trail_h, trail * 0.85);

        let bead_along = (y - fy) * mix(8.0, 15.0, rnd.y) + rnd.x * 6.0;
        let bead_i = floor(bead_along);
        let jitter = (hash21(id + vec2<f32>(bead_i, 1.7)) - 0.5) * 0.022;
        let bead_q = vec2<f32>(
            (px - (col + cx) + jitter) * stretch.x * 1.35,
            (fract(bead_along) - 0.5) * stretch.y * 0.42,
        );
        let bead_gate = trail * smoothstep(0.10, 0.50, dist_up)
            * step(0.35, hash21(id + vec2<f32>(bead_i, 2.3)));
        let bead_rad = 0.011 * mass * bead_gate;
        sd = smin(
            sd,
            length(bead_q) - max(bead_rad, 1e-4) + step(bead_gate, 0.001) * 0.5,
            mix(0.001, 0.012, bead_gate),
        );
    }
    return vec2<f32>(height_from_sd(sd, 0.055), trail_h);
}

fn drops(uv: vec2<f32>, st: vec2<f32>, aspect: f32, t: f32, rain: f32) -> vec2<f32> {
    let static_amt = smoothstep(RAIN_OVERLAY_MIN, 0.40, rain);
    let trail1 = smoothstep(0.35, 0.65, rain);
    let trail2 = smoothstep(0.55, 0.90, rain);
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
    let trail = max(d1.y, d2.y);
    let eaten = mix(0.06, 1.0, 1.0 - smoothstep(0.06, 0.38, trail));
    let sliding = max(d1.x, d2.x);
    return vec2<f32>(max(s * eaten, sliding), trail);
}

fn height(d: vec2<f32>) -> f32 {
    return d.x + d.y * 0.40;
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
    let t = u.time;
    let field = drops(uv, st, aspect, t, rain);
    let h = height(field);
    let n = vec2<f32>(dpdx(h), dpdy(h)) * 1.25;

    let uv_r = uv + n * (0.085 * smoothstep(0.05, 0.50, rain));
    let sharp = sky_sample(uv_r);
    let texel = 4.0 / max(u.resolution, vec2<f32>(1.0));
    let blur = sharp * 0.4
        + sky_sample(uv_r + vec2<f32>(texel.x, 0.0)) * 0.15
        + sky_sample(uv_r - vec2<f32>(texel.x, 0.0)) * 0.15
        + sky_sample(uv_r + vec2<f32>(0.0, texel.y)) * 0.15
        + sky_sample(uv_r - vec2<f32>(0.0, texel.y)) * 0.15;
    let lum = dot(blur, vec3<f32>(0.2126, 0.7152, 0.0722));
    let muted = mix(blur, vec3<f32>(lum), 0.14) * vec3<f32>(0.88, 0.90, 0.95);
    let wet = max(field.x, field.y);
    let haze = rain * (1.0 - smoothstep(0.06, 0.32, wet));
    var col = mix(sharp, mix(blur, muted, 0.50), haze);
    col *= 1.0 - field.x * 0.10;

    let rim = clamp(length(n) * 6.0, 0.0, 1.0) * field.x;
    col += vec3<f32>(0.12, 0.14, 0.16) * rim;
    let spec_n = normalize(vec3<f32>(n * 2.4, 0.55));
    let spec_l = normalize(vec3<f32>(-0.25, -0.65, 0.55));
    col += pow(max(dot(spec_n, spec_l), 0.0), 32.0) * field.x * 0.22;

    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
