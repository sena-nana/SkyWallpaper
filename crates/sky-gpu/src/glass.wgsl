// Original MIT WGSL; Heartfelt-class wet glass (lenses, curved trails, frost).
// Not a CC-BY-NC-SA Shadertoy port.

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

fn slide_y(phase: f32, hold: f32) -> f32 {
    let p = clamp(phase, 0.0, 1.0);
    let h = clamp(hold, 0.62, 0.90);
    let creep = 0.05 * (p / max(h, 1e-3));
    let f = clamp((p - h) / max(1.0 - h, 1e-3), 0.0, 1.0);
    let fall = f * f * (1.18 - 0.18 * f);
    return mix(creep, mix(0.05, 1.0, fall), step(h, p));
}

fn lens_h(d: f32, rad: f32) -> f32 {
    let x = clamp(d / max(rad, 1e-4), 0.0, 1.0);
    let b = 1.0 - x * x;
    return b * b;
}

fn path_x(yy: f32, rnd: vec2<f32>) -> f32 {
    let wiggle = sin(yy * 5.8 + rnd.y * 4.2 + sin(yy * 2.9 + rnd.x * 3.1));
    return 0.5 + (rnd.x - 0.5) * 0.34 + wiggle * mix(0.08, 0.22, rnd.y);
}

fn static_drops(st: vec2<f32>, t: f32, amount: f32) -> f32 {
    if (amount <= 0.0) {
        return 0.0;
    }
    let p = st * 24.0;
    let origin = floor(p);
    var h = 0.0;
    let density = mix(0.16, 0.52, amount);
    for (var jy = -1; jy <= 1; jy = jy + 1) {
        for (var ix = -1; ix <= 1; ix = ix + 1) {
            let cell = origin + vec2<f32>(f32(ix), f32(jy));
            let rnd = hash22(cell);
            let vis = smoothstep(0.0, 0.10, density - rnd.x)
                * smoothstep(0.10, 0.48, 0.5 + 0.5 * sin(t * (0.30 + rnd.y * 0.52) + rnd.x * 6.28318));
            let center = cell + 0.16 + rnd.yx * 0.68;
            h = max(h, lens_h(length(p - center), mix(0.09, 0.20, rnd.x)) * vis);
        }
    }
    return h * amount;
}

fn drop_layer(uv: vec2<f32>, aspect: f32, t: f32, cols: f32, rows: f32) -> vec2<f32> {
    let gx = uv.x * cols;
    let origin_c = floor(gx);
    var h = 0.0;
    var wet = 0.0;
    for (var ix = -1; ix <= 1; ix = ix + 1) {
        let col = origin_c + f32(ix);
        let shift = hash21(vec2<f32>(col, 2.7));
        let gy = uv.y * rows + shift;
        let nrow0 = floor(gy);
        for (var jy = -1; jy <= 1; jy = jy + 1) {
            let nrow = nrow0 + f32(jy);
            let rnd = hash22(vec2<f32>(col, nrow));
            let hold = mix(0.70, 0.86, rnd.x);
            let spd = mix(0.13, 0.38, rnd.y);
            let phase = fract(t * spd + rnd.x * 0.97);
            let y = slide_y(phase, hold);
            let fade = smoothstep(0.0, 0.04, phase) * smoothstep(1.0, 0.90, phase);
            let falling = smoothstep(hold - 0.04, hold + 0.12, phase);
            let st_x = gx - col;
            let st_y = gy - nrow;
            let px = path_x(y, rnd);
            let tx = path_x(st_y, rnd);
            let dx = (st_x - px) * (aspect / max(cols, 1.0));
            let dy = (st_y - y) / max(rows, 1.0);
            let mass = 1.0 + falling * mix(0.16, 0.50, rnd.x);
            let rad = mix(0.011, 0.026, rnd.y) * mass * max(fade, 1e-4);
            let drop = lens_h(length(vec2<f32>(dx, dy)), rad) * fade;
            let tdx = (st_x - tx) * (aspect / max(cols, 1.0));
            let dist_up = clamp((y - st_y) / max(y, 1e-3), 0.0, 1.0);
            let along = smoothstep(y + 0.03, y - 0.08, st_y) * smoothstep(-0.05, 0.10, st_y);
            let trail_w = mix(0.0026, 0.0088, rnd.x) * mix(1.25, 0.16, dist_up) * mass;
            let trail = smoothstep(trail_w, trail_w * 0.12, abs(tdx)) * along * falling * fade;
            let bw = sin(st_y * (1.0 - clamp(st_y, 0.0, 1.0)) * mix(80.0, 140.0, rnd.y) + rnd.x * 6.28318);
            let bead_r = mix(0.006, 0.014, rnd.x) * mass;
            let beads = lens_h(
                length(vec2<f32>(tdx, (fract(st_y * 9.0 + rnd.y) - 0.5) / max(rows, 1.0))),
                bead_r,
            ) * max(bw - 0.32, 0.0) * along * falling * fade;
            h = max(h, max(drop, beads));
            wet = max(wet, trail * 0.9);
        }
    }
    return vec2<f32>(h, wet);
}

fn drops(uv: vec2<f32>, aspect: f32, t: f32, rain: f32) -> vec2<f32> {
    let st = vec2<f32>(uv.x * aspect, uv.y);
    let static_amt = smoothstep(RAIN_OVERLAY_MIN, 0.42, rain);
    let trail1 = smoothstep(0.22, 0.58, rain);
    let trail2 = smoothstep(0.45, 0.85, rain);
    let speed = mix(0.40, 1.20, rain);
    let s = static_drops(st, u.time * 0.45, static_amt);
    var d1 = vec2<f32>(0.0);
    var d2 = vec2<f32>(0.0);
    if (trail1 > 0.0) {
        d1 = drop_layer(uv, aspect, t * speed, 7.0, 1.0) * trail1;
    }
    if (trail2 > 0.0) {
        d2 = drop_layer(uv + vec2<f32>(0.17, 0.31), aspect, t * speed * 1.15, 12.0, 1.85) * trail2;
    }
    let wet = max(d1.y, d2.y);
    let eaten = mix(0.06, 1.0, 1.0 - smoothstep(0.04, 0.32, wet));
    let sliding = max(d1.x, d2.x);
    return vec2<f32>(max(s * eaten, sliding), wet);
}

fn height_of(field: vec2<f32>) -> f32 {
    return field.x + field.y * 0.22;
}

fn sky_sample(uv: vec2<f32>) -> vec3<f32> {
    let p = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(sky_tex, sky_samp, p).rgb;
}

fn frost_blur(uv: vec2<f32>) -> vec3<f32> {
    let t = 10.0 / max(u.resolution, vec2<f32>(1.0));
    let t2 = 22.0 / max(u.resolution, vec2<f32>(1.0));
    var c = sky_sample(uv) * 0.20;
    c += sky_sample(uv + vec2<f32>(t.x, 0.0)) * 0.10;
    c += sky_sample(uv - vec2<f32>(t.x, 0.0)) * 0.10;
    c += sky_sample(uv + vec2<f32>(0.0, t.y)) * 0.10;
    c += sky_sample(uv - vec2<f32>(0.0, t.y)) * 0.10;
    c += sky_sample(uv + vec2<f32>(t.x, t.y)) * 0.07;
    c += sky_sample(uv + vec2<f32>(-t.x, t.y)) * 0.07;
    c += sky_sample(uv + vec2<f32>(t.x, -t.y)) * 0.07;
    c += sky_sample(uv + vec2<f32>(-t.x, -t.y)) * 0.07;
    c += sky_sample(uv + vec2<f32>(t2.x, 0.0)) * 0.03;
    c += sky_sample(uv - vec2<f32>(t2.x, 0.0)) * 0.03;
    c += sky_sample(uv + vec2<f32>(0.0, t2.y)) * 0.03;
    c += sky_sample(uv - vec2<f32>(0.0, t2.y)) * 0.03;
    return c;
}

@fragment
fn fs_main(@builtin(position) clip: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = clip.xy / max(u.resolution, vec2<f32>(1.0));
    let rain = clamp(u.precip * (1.0 - u.precip_kind), 0.0, 1.0);
    if (rain <= RAIN_OVERLAY_MIN) {
        return vec4<f32>(sky_sample(uv), 1.0);
    }

    let aspect = u.resolution.x / max(u.resolution.y, 1.0);
    let t = u.time;
    let field = drops(uv, aspect, t, rain);
    let e = 2.0 / max(u.resolution, vec2<f32>(1.0));
    let h = height_of(field);
    let n0 = vec2<f32>(
        height_of(drops(uv + vec2<f32>(e.x, 0.0), aspect, t, rain)) - h,
        height_of(drops(uv + vec2<f32>(0.0, e.y), aspect, t, rain)) - h,
    ) * mix(1.3, 2.4, rain);
    let n = vec2<f32>(n0.x * 0.92 - n0.y * 0.28, n0.x * 0.28 + n0.y * 0.92);

    let cover = max(
        smoothstep(0.02, 0.18, field.x),
        smoothstep(0.04, 0.32, field.y),
    );
    let sharp = sky_sample(uv + n);
    let blur = frost_blur(uv);
    let lum = dot(blur, vec3<f32>(0.2126, 0.7152, 0.0722));
    let frosted = mix(blur, vec3<f32>(lum), 0.50) * vec3<f32>(0.60, 0.66, 0.76)
        + vec3<f32>(0.018, 0.022, 0.030);
    let frost_amt = mix(0.40, 0.86, rain) * (1.0 - cover);
    var col = mix(sharp, frosted, frost_amt);

    let spec_n = normalize(vec3<f32>(n * 1.6, 0.72));
    col += pow(max(dot(spec_n, normalize(vec3<f32>(-0.22, -0.72, 0.58))), 0.0), 40.0)
        * field.x
        * 0.055
        * smoothstep(0.02, 0.10, length(n));

    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
