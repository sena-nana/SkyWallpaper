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

fn slide_y(phase: f32, hold: f32, stick: f32, travel: f32) -> f32 {
    let p = clamp(phase, 0.0, 1.0);
    let h = clamp(hold, 0.78, 0.92);
    let s = clamp(stick, 0.08, 0.45);
    let tr = clamp(travel, 0.22, 0.55);
    let creep = s * (0.70 + 0.30 * p / max(h, 1e-3));
    let f = clamp((p - h) / max(1.0 - h, 1e-3), 0.0, 1.0);
    let fall = f * f * (1.15 - 0.15 * f);
    return mix(creep, s + tr * fall, step(h, p));
}

fn path_x(yy: f32, rnd: vec2<f32>) -> f32 {
    let x0 = (rnd.x - 0.5) * 0.55;
    let wiggle = sin(yy * 2.2 + rnd.y * 1.4 + sin(yy * 1.3 + rnd.x));
    let amp = mix(0.16, 0.38, rnd.y);
    return 0.5 + x0 + wiggle * amp * (0.55 + 0.9 * (0.5 - abs(x0)));
}

const LENS_REF_RAD: f32 = 0.018;

fn path_dx(yy: f32, rnd: vec2<f32>) -> f32 {
    let x0 = (rnd.x - 0.5) * 0.55;
    let inner = yy * 1.3 + rnd.x;
    let arg = yy * 2.2 + rnd.y * 1.4 + sin(inner);
    let amp = mix(0.16, 0.38, rnd.y) * (0.55 + 0.9 * (0.5 - abs(x0)));
    return cos(arg) * (2.2 + 1.3 * cos(inner)) * amp;
}

fn sm(e0: f32, e1: f32, v: f32) -> vec2<f32> {
    let d = e1 - e0;
    let den = select(d, 1e-5, abs(d) < 1e-5);
    let t = clamp((v - e0) / den, 0.0, 1.0);
    return vec2<f32>(t * t * (3.0 - 2.0 * t), 6.0 * t * (1.0 - t) / den);
}

fn ridge_grad(tdx: f32, rad: f32, vis: f32, dtdx_dy: f32, dvis_dy: f32, dr_dy: f32) -> vec3<f32> {
    let r = max(rad, 1e-4);
    let x = tdx / r;
    if (abs(x) >= 1.0 || vis <= 0.0) {
        return vec3<f32>(0.0);
    }
    let b = 1.0 - x * x;
    let shape = r / LENS_REF_RAD;
    let h = b * b * vis * shape;
    let dhdd = -4.0 * x * b / LENS_REF_RAD * vis;
    let dh_dr = vis / LENS_REF_RAD * b * (1.0 + 3.0 * x * x);
    return vec3<f32>(h, dhdd, dhdd * dtdx_dy + b * b * shape * dvis_dy + dh_dr * dr_dy);
}

fn lens_grad(offset: vec2<f32>, rad: f32, vis: f32) -> vec3<f32> {
    let r = max(rad, 1e-4);
    let d = length(offset);
    let x = d / r;
    if (x >= 1.0 || vis <= 0.0) {
        return vec3<f32>(0.0);
    }
    let b = 1.0 - x * x;
    let shape = r / LENS_REF_RAD;
    let h = b * b * vis * shape;
    if (d < 1e-6) {
        return vec3<f32>(h, 0.0, 0.0);
    }
    let dhdd = -4.0 * x * b / LENS_REF_RAD * vis;
    return vec3<f32>(h, dhdd * offset.x / d, dhdd * offset.y / d);
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
    let scale = 24.0;
    let p = st * scale;
    let origin = floor(p);
    var f = vec3<f32>(0.0);
    let density = mix(0.16, 0.52, amount);
    for (var jy = -1; jy <= 1; jy = jy + 1) {
        for (var ix = -1; ix <= 1; ix = ix + 1) {
            let cell = origin + vec2<f32>(f32(ix), f32(jy));
            let rnd = hash22(cell);
            let vis = smoothstep(0.0, 0.10, density - rnd.x)
                * smoothstep(0.10, 0.48, 0.5 + 0.5 * sin(t * (0.12 + rnd.y * 0.22) + rnd.x * 6.28318))
                * amount;
            let center = cell + 0.16 + rnd.yx * 0.68;
            let rad = mix(0.09, 0.20, rnd.x) / scale;
            f = smax3(f, lens_grad((p - center) / scale, rad, vis));
        }
    }
    return f;
}

fn drop_layer(uv: vec2<f32>, aspect: f32, t: f32, cols: f32, rows: f32) -> vec4<f32> {
    let scrolled = vec2<f32>(uv.x, uv.y - t * 0.42);
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
            let rndb = hash22(vec2<f32>(col + 3.1, nrow + 8.7));
            let hold = mix(0.80, 0.90, rnd.x);
            let spd = mix(0.10, 0.26, rnd.y);
            let phase = fract(t * spd + rnd.x * 0.97);
            let stick = mix(0.10, 0.38, rndb.x);
            let y = slide_y(phase, hold, stick, mix(0.24, 0.48, rndb.y));
            let vis = smoothstep(0.0, 0.14, phase) * smoothstep(1.0, 0.90, phase);
            let falling = smoothstep(hold - 0.02, hold + 0.08, phase);
            let grow = mix(0.50, 1.08, clamp(phase / max(hold, 1e-3), 0.0, 1.0));
            let st_x = gx - col;
            let st_y = gy - nrow;
            let px = path_x(y, rnd);
            let tx = path_x(st_y, rnd);
            let mass = grow + falling * mix(0.06, 0.22, rnd.x);
            let rad = mix(0.010, 0.024, rnd.y) * mass * max(vis, 1e-4);
            let drop = lens_grad(vec2<f32>((st_x - px) * sx, (st_y - y) * sy), rad, vis);
            let tdx = (st_x - tx) * sx;
            let span = max(y - stick, 1e-3);
            let dist_raw = (y - st_y) / span;
            let dist_up = clamp(dist_raw, 0.0, 1.0);
            let cap = sm(stick - 0.04, stick + 0.06, st_y);
            let head = sm(y + 0.04, y - 0.08, st_y);
            let along = cap.x * head.x;
            let w0 = mix(0.006, 0.016, rnd.x);
            let trail_w = w0 * mix(1.15, 0.28, dist_up) * mass;
            let life = vis * falling;
            let trail_vis = along * life;
            let dtdx_dy = -path_dx(st_y, rnd) * rows * sx;
            let dvis_dy = (cap.y * head.x + cap.x * head.y) * rows * life;
            let tapering = select(0.0, 1.0, dist_raw > 0.0 && dist_raw < 1.0);
            let dr_dy = tapering * w0 * mass * (0.28 - 1.15) * (-rows / span);
            let trail = ridge_grad(tdx, trail_w, trail_vis, dtdx_dy, dvis_dy, dr_dy);
            f = smax3(f, drop);
            f = smax3(f, trail * vec3<f32>(0.55, 0.55, 0.55));
            wet = max(wet, trail_vis * mix(0.45, 1.0, 1.0 - dist_up));
            let n_bead = 8.0;
            let slot0 = floor(st_y * n_bead);
            for (var kb = -1; kb <= 1; kb = kb + 1) {
                let slot = slot0 + f32(kb);
                if (slot < 0.0 || slot >= n_bead || vis <= 0.0 || falling <= 0.0) {
                    continue;
                }
                let br = hash22(vec2<f32>(col + 4.3, nrow + slot * 1.9));
                let by = (slot + 0.22 + br.x * 0.56) / n_bead;
                let sprinkle = sin(by * (1.0 - by) * 36.0 + rnd.x * 6.28318);
                let passed = smoothstep(by - 0.03, by + 0.02, y)
                    * smoothstep(stick - 0.02, stick + 0.06, by);
                if (sprinkle < 0.18 || passed <= 0.0) {
                    continue;
                }
                let bx = path_x(by, rnd);
                let brad = mix(0.0035, 0.010, br.y) * mass;
                let bead = lens_grad(
                    vec2<f32>((st_x - bx) * sx, (st_y - by) * sy),
                    brad,
                    vis * passed * mix(0.40, 0.90, br.y) * smoothstep(0.18, 0.55, sprinkle),
                );
                f = smax3(f, bead);
            }
        }
    }
    return vec4<f32>(f, wet);
}

fn drops(uv: vec2<f32>, aspect: f32, t: f32, rain: f32) -> vec4<f32> {
    let st = vec2<f32>(uv.x * aspect, uv.y);
    let static_amt = smoothstep(RAIN_OVERLAY_MIN, 0.42, rain);
    let trail1 = smoothstep(0.22, 0.58, rain);
    let trail2 = smoothstep(0.45, 0.85, rain);
    let speed = mix(0.35, 0.90, rain);
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

fn sky_sample(uv: vec2<f32>) -> vec3<f32> {
    let p = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(sky_tex, sky_samp, p).rgb;
}

fn frost_blur(uv: vec2<f32>) -> vec3<f32> {
    let t = 12.0 / max(u.resolution, vec2<f32>(1.0));
    var c = sky_sample(uv) * 0.20;
    c += sky_sample(uv + vec2<f32>(t.x, 0.0)) * 0.12;
    c += sky_sample(uv - vec2<f32>(t.x, 0.0)) * 0.12;
    c += sky_sample(uv + vec2<f32>(0.0, t.y)) * 0.12;
    c += sky_sample(uv - vec2<f32>(0.0, t.y)) * 0.12;
    c += sky_sample(uv + vec2<f32>(t.x, t.y)) * 0.08;
    c += sky_sample(uv + vec2<f32>(-t.x, t.y)) * 0.08;
    c += sky_sample(uv + vec2<f32>(t.x, -t.y)) * 0.08;
    c += sky_sample(uv + vec2<f32>(-t.x, -t.y)) * 0.08;
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
    let field = drops(uv, aspect, u.time, rain);
    let ior = mix(0.0008, 0.0022, rain);
    let n = vec2<f32>(field.y * ior / max(aspect, 1e-4), field.z * ior);
    let warped = uv + n;
    let sharp = sky_sample(warped);
    let blur = frost_blur(warped);
    let sharp_amt = smoothstep(0.0, 0.45, max(field.x, field.w));
    let frost = mix(0.48, 0.80, rain) * (1.0 - sharp_amt);
    let col = mix(sharp, blur, frost);
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
