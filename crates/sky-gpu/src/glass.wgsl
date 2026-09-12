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

fn hash22(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(hash21(p), hash21(p + vec2<f32>(19.2, 8.4)));
}

fn slide_y(phase: f32) -> f32 {
    let hold = 0.38;
    let p = clamp(phase, 0.0, 1.0);
    let stuck = 0.05 * (p / hold);
    let f = clamp((p - hold) / (1.0 - hold), 0.0, 1.0);
    let fall = f * f * (1.15 - 0.15 * f);
    if (p < hold) {
        return stuck;
    }
    return mix(0.05, 1.0, clamp(fall, 0.0, 1.0));
}

fn static_drops(st: vec2<f32>, t: f32, amount: f32) -> f32 {
    if (amount <= 0.0) {
        return 0.0;
    }
    let scale = 22.0;
    let p = st * scale;
    let origin = floor(p);
    var h = 0.0;
    for (var jy = -1; jy <= 1; jy = jy + 1) {
        for (var ix = -1; ix <= 1; ix = ix + 1) {
            let cell = origin + vec2<f32>(f32(ix), f32(jy));
            let rnd = hash22(cell);
            let density = 0.48 * amount;
            if (rnd.x <= density) {
                let center = cell + vec2<f32>(0.18, 0.18) + hash22(cell + vec2<f32>(3.1, 7.7)) * 0.64;
                let life = 0.5 + 0.5 * sin(t * (0.32 + rnd.y * 0.55) + rnd.x * 6.28318);
                let vis = smoothstep(0.10, 0.52, life);
                let rad = mix(0.045, 0.13, fract(rnd.x * 13.0)) * vis;
                let d = length(p - center);
                h = max(h, smoothstep(rad, rad * 0.12, d) * vis);
            }
        }
    }
    return h * amount;
}

fn drop_layer(uv: vec2<f32>, aspect: f32, t: f32, cols: f32, rows: f32) -> vec2<f32> {
    var p = vec2<f32>(uv.x * cols, uv.y * rows);
    let col = floor(p.x);
    p.y += hash21(vec2<f32>(col, 2.7));
    let id = floor(p);
    let f = fract(p);
    let rnd = hash22(id);

    let phase = fract(t * mix(0.16, 0.40, rnd.y) + rnd.x);
    let y = slide_y(phase);
    let fade = smoothstep(0.0, 0.05, phase) * smoothstep(1.0, 0.88, phase);
    let wig = sin(phase * 16.0 + rnd.y * 9.0) * 0.05 * smoothstep(0.38, 0.78, phase);
    let cx = 0.5 + (rnd.x - 0.5) * 0.40 + wig;

    let stretch = vec2<f32>(aspect / max(cols, 1.0), 1.0 / max(rows, 1.0));
    var q = (f - vec2<f32>(cx, y)) * stretch;
    q.x *= 1.30;
    q.y *= 0.70;
    let rad = mix(0.010, 0.022, rnd.y);
    let drop = smoothstep(rad, rad * 0.18, length(q)) * fade;

    let falling = smoothstep(0.38, 0.55, phase);
    let trail_w = mix(0.0030, 0.0075, rnd.x);
    let along = smoothstep(y, y - 0.02, f.y) * smoothstep(0.0, 0.06, f.y);
    let dx = abs(f.x - cx) * stretch.x;
    let trail = smoothstep(trail_w, trail_w * 0.18, dx) * along * falling * fade;

    let spaced = fract((y - f.y) * mix(10.0, 16.0, rnd.y) + rnd.x * 5.0);
    let bead_q = vec2<f32>((f.x - cx) * stretch.x * 1.5, (spaced - 0.5) * stretch.y * 0.35);
    let bead = smoothstep(0.010, 0.0, length(bead_q)) * trail * 0.9;

    return vec2<f32>(max(drop, bead), trail * 0.85);
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
    return vec2<f32>(max(s, max(d1.x, d2.x)), max(d1.y, d2.y));
}

fn height(d: vec2<f32>) -> f32 {
    return d.x + d.y * 0.40;
}

fn sky_sample(uv: vec2<f32>) -> vec3<f32> {
    let p = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(sky_tex, sky_samp, p).rgb
        + lightning_rgb(p, u.thunder) * (1.0 - sky_fog_amt(p, u.fog));
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
    let e = 1.6 / max(u.resolution.y, 1.0);
    let n = vec2<f32>(
        height(drops(uv + vec2<f32>(e, 0.0), st + vec2<f32>(e * aspect, 0.0), aspect, t, rain)) - height(field),
        height(drops(uv + vec2<f32>(0.0, e), st + vec2<f32>(0.0, e), aspect, t, rain)) - height(field),
    );

    let uv_r = uv + n * (0.10 * smoothstep(0.05, 0.50, rain));
    let sharp = sky_sample(uv_r);
    let texel = 4.0 / max(u.resolution, vec2<f32>(1.0));
    let blur = sharp * 0.4
        + sky_sample(uv_r + vec2<f32>(texel.x, 0.0)) * 0.15
        + sky_sample(uv_r - vec2<f32>(texel.x, 0.0)) * 0.15
        + sky_sample(uv_r + vec2<f32>(0.0, texel.y)) * 0.15
        + sky_sample(uv_r - vec2<f32>(0.0, texel.y)) * 0.15;
    let lum = dot(blur, vec3<f32>(0.2126, 0.7152, 0.0722));
    let muted = mix(blur, vec3<f32>(lum), 0.14) * vec3<f32>(0.88, 0.90, 0.95);
    let haze = rain * (1.0 - smoothstep(0.08, 0.28, field.x));
    var col = mix(sharp, mix(blur, muted, 0.50), haze);

    let rim = clamp(length(n) * 6.0, 0.0, 1.0) * field.x;
    col += vec3<f32>(0.12, 0.14, 0.16) * rim;
    let spec_n = normalize(vec3<f32>(n * 2.4, 0.55));
    let spec_l = normalize(vec3<f32>(-0.25, -0.65, 0.55));
    col += pow(max(dot(spec_n, spec_l), 0.0), 32.0) * field.x * 0.22;

    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
