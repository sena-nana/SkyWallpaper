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

const LENS_REF_RAD: f32 = 0.018;

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

fn path_x(yy: f32, rnd: vec2<f32>) -> f32 {
    let x0 = (rnd.x - 0.5) * 0.50;
    let inner = yy * 0.55 + rnd.x;
    let arg = yy * 0.85 + rnd.y * 0.8 + sin(inner);
    let amp = mix(0.05, 0.14, rnd.y) * (0.55 + 0.70 * (0.5 - abs(x0)));
    return 0.5 + x0 + sin(arg) * amp;
}

fn bump(x: f32, vis: f32, r: f32) -> vec2<f32> {
    if (abs(x) >= 1.0 || vis <= 0.0) {
        return vec2<f32>(0.0);
    }
    let b = 1.0 - x * x;
    let k = vis / LENS_REF_RAD;
    return vec2<f32>(b * b * k * r, -4.0 * x * b * k);
}

fn lens_grad(offset: vec2<f32>, rad: f32, vis: f32) -> vec3<f32> {
    let r = max(rad, 1e-4);
    let d = length(offset);
    let p = bump(d / r, vis, r);
    if (d < 1e-6) {
        return vec3<f32>(p.x, 0.0, 0.0);
    }
    return vec3<f32>(p.x, p.y * offset.x / d, p.y * offset.y / d);
}

fn smax3(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    if (b.x <= 0.0) {
        return a;
    }
    if (a.x <= 0.0) {
        return b;
    }
    let k = 0.045;
    let w = clamp(0.5 + 0.5 * (b.x - a.x) / k, 0.0, 1.0);
    return vec3<f32>(
        mix(a.x, b.x, w) + k * w * (1.0 - w),
        mix(a.yz, b.yz, w),
    );
}

fn static_drops(st: vec2<f32>, t: f32, amount: f32) -> vec3<f32> {
    if (amount <= 0.0) {
        return vec3<f32>(0.0);
    }
    let scale = 22.0;
    let p = st * scale;
    let origin = floor(p);
    var f = vec3<f32>(0.0);
    let density = mix(0.26, 0.62, amount);
    for (var jy = -1; jy <= 1; jy = jy + 1) {
        for (var ix = -1; ix <= 1; ix = ix + 1) {
            let cell = origin + vec2<f32>(f32(ix), f32(jy));
            let rnd = hash22(cell);
            let vis = smoothstep(0.0, 0.08, density - rnd.x)
                * smoothstep(0.12, 0.52, 0.5 + 0.5 * sin(t * (0.10 + rnd.y * 0.18) + rnd.x * 6.28318))
                * amount;
            let center = cell + 0.16 + rnd.yx * 0.68;
            let rad = mix(0.11, 0.26, rnd.x) / scale;
            f = smax3(f, lens_grad((p - center) / scale, rad, vis));
        }
    }
    return f;
}

fn drop_layer(uv: vec2<f32>, aspect: f32, t: f32, cols: f32, rows: f32) -> vec4<f32> {
    let scrolled = vec2<f32>(uv.x, uv.y - t * 0.55);
    let gx = scrolled.x * cols;
    let origin_c = floor(gx);
    var f = vec3<f32>(0.0);
    var wet = 0.0;
    let sx = aspect / max(cols, 1.0);
    let sy = 1.0 / max(rows, 1.0);
    for (var ix = -1; ix <= 1; ix = ix + 1) {
        let col = origin_c + f32(ix);
        let shift = hash21(vec2<f32>(col, 2.7));
        let gy = scrolled.y * rows + shift;
        let nrow0 = floor(gy);
        for (var jy = -1; jy <= 1; jy = jy + 1) {
            let nrow = nrow0 + f32(jy);
            let rnd = hash22(vec2<f32>(col, nrow));
            let split = mix(0.80, 0.90, rnd.x);
            let phase = fract(t * mix(0.12, 0.28, rnd.y) + rnd.x * 0.97);
            let vis = smoothstep(0.0, 0.07, phase) * smoothstep(split + 0.02, split, phase);
            if (vis <= 0.0) {
                continue;
            }
            let y = mix(0.07, 0.90, smoothstep(0.0, split, phase));
            let st_x = gx - col;
            let st_y = gy - nrow;
            let path0 = uv.y * 7.0 + hash21(vec2<f32>(col + 3.1, nrow + 8.7));
            let mass = mix(0.88, 1.18, rnd.x);
            f = smax3(f, lens_grad(
                vec2<f32>((st_x - path_x(path0 + y, rnd)) * sx, (st_y - y) * sy),
                mix(0.016, 0.034, rnd.y) * mass,
                vis,
            ));
            let r_env = sqrt(clamp(st_y / max(y, 1e-3), 0.0, 1.0));
            let trail_vis = vis
                * smoothstep(-0.03, 0.05, st_y)
                * smoothstep(y + 0.06, y - 0.03, st_y);
            if (trail_vis > 0.0) {
                let rad = mix(0.08, 0.13, rnd.x) * sx * mass * mix(0.22, 1.0, r_env);
                let cd = abs((st_x - path_x(path0 + st_y, rnd)) * sx);
                wet = max(wet, trail_vis * smoothstep(max(rad, 1e-4), 0.0, cd) * mix(0.50, 1.0, r_env));
            }
            let slot0 = floor(st_y * 8.0);
            for (var kb = -1; kb <= 1; kb = kb + 1) {
                let slot = slot0 + f32(kb);
                if (slot < 0.0 || slot >= 8.0) {
                    continue;
                }
                let br = hash22(vec2<f32>(col + 4.3, nrow + slot * 1.9));
                let by = (slot + 0.22 + br.x * 0.56) / 8.0;
                let sprinkle = sin(by * (1.0 - by) * 36.0 + rnd.x * 6.28318);
                let passed = smoothstep(by - 0.03, by + 0.02, y) * smoothstep(0.02, 0.08, by);
                if (sprinkle < 0.16 || passed <= 0.0) {
                    continue;
                }
                f = smax3(f, lens_grad(
                    vec2<f32>((st_x - path_x(path0 + by, rnd)) * sx, (st_y - by) * sy),
                    mix(0.004, 0.012, br.y) * mass,
                    vis * passed * mix(0.40, 0.90, br.y) * smoothstep(0.16, 0.52, sprinkle),
                ));
            }
        }
    }
    return vec4<f32>(f, wet);
}

fn drops(uv: vec2<f32>, aspect: f32, t: f32, rain: f32) -> vec4<f32> {
    let st = vec2<f32>(uv.x * aspect, uv.y);
    let static_amt = smoothstep(RAIN_OVERLAY_MIN, 0.36, rain);
    let trail1 = smoothstep(0.22, 0.58, rain);
    let trail2 = smoothstep(0.42, 0.82, rain);
    let speed = mix(0.32, 0.82, rain);
    var s = static_drops(st, u.time * 0.45, static_amt);
    var d1 = vec4<f32>(0.0);
    var d2 = vec4<f32>(0.0);
    if (trail1 > 0.0) {
        d1 = drop_layer(uv, aspect, t * speed, 6.0, 2.0) * trail1;
    }
    if (trail2 > 0.0) {
        d2 = drop_layer(uv + vec2<f32>(0.17, 0.31), aspect, t * speed * 1.12, 12.0, 2.0) * trail2;
    }
    let wet = max(d1.w, d2.w);
    let eaten = mix(0.06, 1.0, 1.0 - smoothstep(0.04, 0.32, wet));
    s = s * eaten;
    let sliding = smax3(d1.xyz, d2.xyz);
    let h = smax3(s, sliding);
    return vec4<f32>(h, wet);
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
    let field = drops(uv, aspect, u.time, rain);
    let ior = mix(0.005, 0.012, rain);
    let n = vec2<f32>(field.y * ior / max(aspect, 1e-4), field.z * ior);
    let warped = uv + n;
    let sharp = sky_sample(warped);
    let blur = frost_blur(warped);
    let lum = luma3(blur);
    let milk = mix(blur, vec3<f32>(lum) * vec3<f32>(0.90, 0.94, 1.02) + vec3<f32>(0.055), 0.42);
    let drop = smoothstep(0.10, 0.22, field.x);
    let max_frost = mix(0.62, 0.88, rain) * (1.0 - clamp(field.w, 0.0, 1.0));
    let frost = mix(max_frost, 0.0, drop);
    var col = mix(sharp, milk, frost);

    let N = normalize(vec3<f32>(-n.x, -n.y, 0.28));
    let L = normalize(vec3<f32>(u.sun_dir.x, -u.sun_dir.y, max(u.sun_dir.z, 0.18)));
    let H = normalize(L + vec3<f32>(0.0, 0.0, 1.0));
    let spec = pow(max(dot(N, H), 0.0), 52.0) * drop;
    let sun_l = smoothstep(-0.04, 0.28, u.sun_dir.y);
    col += spec * mix(0.07, 0.26, sun_l) * mix(vec3<f32>(1.0, 0.90, 0.72), vec3<f32>(1.0), sun_l);

    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
