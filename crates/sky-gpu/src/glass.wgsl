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

fn luma3(c: vec3<f32>) -> f32 {
    return 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y,
    );
}

fn saw01(b: f32, t: f32) -> f32 {
    return smoothstep(0.0, b, t) * smoothstep(1.0, b, t);
}

fn sd_egg(p: vec2<f32>, ra: f32, rb: f32) -> f32 {
    let k = sqrt(3.0);
    var q = p;
    q.x = abs(q.x);
    let r = ra - rb;
    var d: f32;
    if (q.y < 0.0) {
        d = length(q) - r;
    } else if (k * (q.x + r) < q.y) {
        d = length(vec2<f32>(q.x, q.y - k * r));
    } else {
        d = length(vec2<f32>(q.x + r, q.y)) - 2.0 * r;
    }
    return d - rb;
}

fn static_drops(uv: vec2<f32>, t: f32, amount: f32) -> f32 {
    if (amount <= 0.0) {
        return 0.0;
    }
    let p = uv * 40.0;
    let origin = floor(p);
    var acc = 0.0;
    for (var jy = -1; jy <= 1; jy = jy + 1) {
        for (var ix = -1; ix <= 1; ix = ix + 1) {
            let cell = origin + vec2<f32>(f32(ix), f32(jy));
            let rnd = hash22(cell);
            let fade = saw01(0.10, fract(t + rnd.y));
            let center = (rnd - 0.5) * 0.6;
            let q = p - cell - 0.5;
            let drop = smoothstep(0.3, 0.0, length(q - center));
            acc += drop * fade * fract(rnd.x * 27.0) * amount;
        }
    }
    return acc;
}

fn drop_layer(uv: vec2<f32>, t: f32) -> vec2<f32> {
    let a = vec2<f32>(6.0, 1.0);
    let grid = a * 2.0;
    let gx = uv.x * grid.x;
    let origin_c = floor(gx);
    var m = 0.0;
    var trail = 0.0;
    for (var ix = -1; ix <= 1; ix = ix + 1) {
        let col = origin_c + f32(ix);
        let grid_fall = hash21(vec2<f32>(col, 0.17)) * 0.333 + 0.5;
        var qy = uv.y - t * grid_fall / a.y;
        qy += hash21(vec2<f32>(col, 1.31));
        let gy = qy * grid.y;
        let nrow0 = floor(gy);
        for (var jy = -1; jy <= 1; jy = jy + 1) {
            let nrow = nrow0 + f32(jy);
            let rnd = hash22(vec2<f32>(col, nrow));
            let rndz = hash21(vec2<f32>(col + 3.1, nrow + 8.7));
            let st = vec2<f32>(gx - col, gy - nrow) - vec2<f32>(0.5, 0.0);
            var x = rnd.x - 0.5;
            let wiggle = sin(qy * 20.0 + sin(qy * 20.0));
            x += wiggle * (0.5 - abs(x)) * (rndz - 0.5) * 0.3;
            x *= 0.6;
            let ti = fract(t * (grid_fall + 0.1) + rndz);
            let y = ti;
            let drop_shape = mix(0.0, -0.2, ti);
            let p = vec2<f32>((st.x - x) * a.y, (y - st.y) * a.x);
            let d = sd_egg(p, 0.0, drop_shape);
            let diameter = hash21(vec2<f32>(col + nrow, 4.2)) / 7.0 + 0.2;
            m = max(m, smoothstep(diameter / 1.5, 0.0, d));
            let r2 = smoothstep(0.0, max(y, 1e-3), st.y);
            let head = smoothstep(y + 0.05, y - 0.05, st.y);
            let width = diameter * 0.75 * sqrt(max(r2, 1e-5));
            let tr = smoothstep(width, 0.0, abs(st.x - x)) * r2 * head * 0.5;
            trail = max(trail, tr);
        }
    }
    return vec2<f32>(m, trail);
}

fn drops(uv: vec2<f32>, t: f32, rain: f32) -> vec2<f32> {
    let static_amt = smoothstep(-0.5, 1.0, rain) * 2.0;
    let layer1 = smoothstep(0.25, 0.75, rain);
    let layer2 = smoothstep(0.0, 0.5, rain);
    let s = static_drops(uv, t, static_amt);
    var m1 = vec2<f32>(0.0);
    var m2 = vec2<f32>(0.0);
    if (layer1 > 0.0) {
        m1 = drop_layer(uv, t) * layer1;
    }
    if (layer2 > 0.0) {
        m2 = drop_layer(uv * 1.85, t) * layer2;
    }
    let c = smoothstep(0.3, 1.0, s + m1.x + m2.x);
    return vec2<f32>(c, m1.y + m2.y);
}

fn sky_texel(uv: vec2<f32>) -> vec3<f32> {
    let p = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(sky_tex, sky_samp, p).rgb;
}

fn sky_sample(uv: vec2<f32>) -> vec3<f32> {
    let g = noise2(uv * 18.0) * 0.65 + noise2(uv * 40.0 + vec2<f32>(4.2, 1.1)) * 0.35;
    return sky_texel(uv) * (1.0 + (g - 0.5) * 0.10);
}

fn frost_blur(uv: vec2<f32>) -> vec3<f32> {
    let t = 22.0 / max(u.resolution, vec2<f32>(1.0));
    var c = sky_texel(uv) * 0.20;
    c += sky_texel(uv + vec2<f32>(t.x, 0.0)) * 0.12;
    c += sky_texel(uv - vec2<f32>(t.x, 0.0)) * 0.12;
    c += sky_texel(uv + vec2<f32>(0.0, t.y)) * 0.12;
    c += sky_texel(uv - vec2<f32>(0.0, t.y)) * 0.12;
    c += sky_texel(uv + vec2<f32>(t.x, t.y)) * 0.08;
    c += sky_texel(uv + vec2<f32>(-t.x, t.y)) * 0.08;
    c += sky_texel(uv + vec2<f32>(t.x, -t.y)) * 0.08;
    c += sky_texel(uv + vec2<f32>(-t.x, -t.y)) * 0.08;
    return c;
}

@fragment
fn fs_main(@builtin(position) clip: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = clip.xy / max(u.resolution, vec2<f32>(1.0));
    let rain = clamp(u.precip * (1.0 - u.precip_kind), 0.0, 1.0);
    if (rain <= RAIN_OVERLAY_MIN) {
        return vec4<f32>(sky_texel(uv), 1.0);
    }

    let aspect = u.resolution.x / max(u.resolution.y, 1.0);
    let auv = vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5);
    let t = u.time * 0.2;
    let field = drops(auv, t, rain);
    let n = vec2<f32>(dpdx(field.x), dpdy(field.x));
    let warped = uv + n;
    let sharp = sky_sample(warped);
    let blur = frost_blur(warped);
    let lum = luma3(blur);
    let milk = mix(blur, vec3<f32>(lum) * vec3<f32>(0.90, 0.94, 1.02) + vec3<f32>(0.055), 0.42);
    let drop = smoothstep(0.10, 0.22, field.x);
    let max_frost = mix(0.62, 0.88, rain) * (1.0 - clamp(field.y, 0.0, 1.0));
    let frost = mix(max_frost, 0.0, drop);
    let col = mix(sharp, milk, frost);
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
