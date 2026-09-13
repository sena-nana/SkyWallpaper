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

const COLS: f32 = 12.0;
const Y_STRETCH: f32 = 12.0;
const SLOTS: i32 = 3;
const PANE_END: f32 = 1.18;

struct Drop {
    valid: f32,
    col: f32,
    slot: f32,
    cycle: f32,
    x0: f32,
    amp: f32,
    y: f32,
    y_spawn: f32,
    r0: f32,
    v: f32,
    t_spawn: f32,
    t_fall: f32,
    t_end: f32,
};

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

fn sd_egg(p: vec2<f32>, rb: f32) -> f32 {
    let k = sqrt(3.0);
    let q = vec2<f32>(abs(p.x), p.y);
    let r = -rb;
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

fn pane_x(uv_x: f32, aspect: f32) -> f32 {
    return (uv_x - 0.5) * aspect;
}

fn slot_need(slot: i32) -> f32 {
    if (slot <= 0) {
        return 0.0;
    }
    if (slot == 1) {
        return 0.30;
    }
    return 0.58;
}

fn cycle_len_of(col: i32, slot: i32) -> f32 {
    return mix(8.0, 13.0, hash21(vec2<f32>(f32(col) * 0.11 + 2.3, f32(slot) * 5.9)));
}

fn phase_of(col: i32, slot: i32) -> f32 {
    return hash21(vec2<f32>(f32(col) + 0.7, f32(slot) * 11.3));
}

fn current_cycle(col: i32, slot: i32, t: f32) -> i32 {
    return i32(floor(t / cycle_len_of(col, slot) + phase_of(col, slot)));
}

fn drop_x(d: Drop, y: f32) -> f32 {
    let w = sin(y * 20.0 + sin(y * 20.0));
    return d.x0 + w * d.amp;
}

fn y_at(d: Drop, t: f32) -> f32 {
    if (t < d.t_fall) {
        return d.y_spawn;
    }
    return d.y_spawn + d.v * (t - d.t_fall);
}

fn vel_at(d: Drop, t: f32) -> f32 {
    if (t < d.t_fall) {
        return 0.0;
    }
    return d.v;
}

fn r_now(d: Drop, t: f32) -> f32 {
    if (d.t_fall > d.t_spawn + 1e-4 && t < d.t_fall) {
        return d.r0 * smoothstep(d.t_spawn, d.t_fall, t);
    }
    return d.r0;
}

fn drop_wins(a: Drop, b: Drop) -> bool {
    if (a.r0 > b.r0 + 1e-6) {
        return true;
    }
    if (b.r0 > a.r0 + 1e-6) {
        return false;
    }
    if (a.col != b.col) {
        return a.col < b.col;
    }
    if (a.slot != b.slot) {
        return a.slot < b.slot;
    }
    return a.cycle < b.cycle;
}

fn same_drop(a: Drop, b: Drop) -> bool {
    return a.col == b.col && a.slot == b.slot && a.cycle == b.cycle;
}

fn lane_close(a: Drop, b: Drop) -> bool {
    return abs(a.x0 - b.x0) <= (a.r0 + b.r0) / COLS + 0.85 / COLS;
}

fn catch_in(a: Drop, b: Drop, t0: f32, t1: f32) -> f32 {
    if (t1 <= t0 + 1e-4) {
        return -1.0;
    }
    let ya = y_at(a, t0);
    let yb = y_at(b, t0);
    let ry = (a.r0 + b.r0) / Y_STRETCH + 0.03;
    if (abs(ya - yb) <= ry) {
        return t0;
    }
    let va = vel_at(a, t0);
    let vb = vel_at(b, t0);
    if (ya < yb && va > vb + 1e-5) {
        let tm = t0 + (yb - ya) / (va - vb);
        if (tm <= t1) {
            return tm;
        }
    } else if (yb < ya && vb > va + 1e-5) {
        let tm = t0 + (ya - yb) / (vb - va);
        if (tm <= t1) {
            return tm;
        }
    }
    return -1.0;
}

fn meet_t(a: Drop, b: Drop, t: f32) -> f32 {
    let t_lo = max(a.t_spawn, b.t_spawn);
    let t_hi = min(min(a.t_end, b.t_end), t);
    if (t_lo >= t_hi || !lane_close(a, b)) {
        return -1.0;
    }
    var t1 = clamp(a.t_fall, t_lo, t_hi);
    var t2 = clamp(b.t_fall, t_lo, t_hi);
    if (t1 > t2) {
        let s = t1;
        t1 = t2;
        t2 = s;
    }
    var hit = catch_in(a, b, t_lo, t1);
    if (hit < 0.0) {
        hit = catch_in(a, b, t1, t2);
    }
    if (hit < 0.0) {
        hit = catch_in(a, b, t2, t_hi);
    }
    return hit;
}

fn drop_at(col: i32, slot: i32, cycle: i32, t: f32, rain: f32) -> Drop {
    var d: Drop;
    d.valid = 0.0;
    d.col = f32(col);
    d.slot = f32(slot);
    d.cycle = f32(cycle);
    d.x0 = 0.0;
    d.amp = 0.0;
    d.y = 0.0;
    d.y_spawn = 0.0;
    d.r0 = 0.0;
    d.v = 0.0;
    d.t_spawn = 0.0;
    d.t_fall = 0.0;
    d.t_end = 0.0;
    if (cycle < 0 || rain < slot_need(slot)) {
        return d;
    }
    let L = cycle_len_of(col, slot);
    let ph = phase_of(col, slot);
    let t_spawn = (f32(cycle) - ph) * L;
    let rnd = hash22(vec2<f32>(f32(col) * 3.1 + f32(cycle) * 17.0, f32(slot) * 8.3 + f32(cycle) * 4.7));
    let rndz = hash21(vec2<f32>(f32(col + slot * 19), f32(cycle) * 9.1 + 2.4));
    let mode_b = rnd.y < mix(0.22, 0.72, rain);
    let u_off = mix(0.78, 0.90, rndz);
    d.x0 = (f32(col) + 0.5 + (rnd.x - 0.5) * 0.55) / COLS;
    d.amp = (0.5 - abs(rnd.x - 0.5)) * (rndz - 0.5) * 0.3 / COLS;
    d.r0 = mix(0.16, 0.34, fract(rnd.x + rnd.y * 3.7));
    d.t_spawn = t_spawn;
    if (mode_b) {
        d.y_spawn = -0.07;
        d.t_fall = t_spawn;
        let dur = max(u_off * L, 0.2);
        d.v = (PANE_END - d.y_spawn) / dur;
        d.t_end = t_spawn + dur;
    } else {
        let u_grow = mix(0.10, 0.22, fract(rndz * 5.1));
        d.y_spawn = mix(0.04, 0.62, fract(rnd.y + rndz));
        d.t_fall = t_spawn + u_grow * L;
        let dur = max((u_off - u_grow) * L, 0.2);
        d.v = (PANE_END - d.y_spawn) / dur;
        d.t_end = d.t_fall + dur;
    }
    d.y = y_at(d, t);
    d.valid = 1.0;
    return d;
}

fn eat_info(me: Drop, t: f32, rain: f32) -> vec2<f32> {
    var swallowed = 0.0;
    var extra = 0.0;
    let col = i32(me.col);
    for (var dc = -2; dc <= 2; dc = dc + 1) {
        for (var slot = 0; slot < SLOTS; slot = slot + 1) {
            let oc = current_cycle(col + dc, slot, t);
            for (var g = 0; g <= 1; g = g + 1) {
                let other = drop_at(col + dc, slot, oc - g, t, rain);
                if (other.valid < 0.5 || same_drop(me, other)) {
                    continue;
                }
                let tm = meet_t(me, other, t);
                if (tm < 0.0) {
                    continue;
                }
                if (drop_wins(other, me)) {
                    swallowed = 1.0;
                } else if (drop_wins(me, other)) {
                    extra += other.r0 * other.r0 * smoothstep(tm, tm + 0.28, t);
                }
            }
        }
    }
    return vec2<f32>(swallowed, extra);
}

fn drops(uv: vec2<f32>, aspect: f32, t: f32, rain: f32) -> vec2<f32> {
    let px = pane_x(uv.x, aspect);
    let col0 = i32(floor(px * COLS));
    var m = 0.0;
    var trail = 0.0;
    for (var dc = -1; dc <= 1; dc = dc + 1) {
        for (var slot = 0; slot < SLOTS; slot = slot + 1) {
            let cyc = current_cycle(col0 + dc, slot, t);
            let d = drop_at(col0 + dc, slot, cyc, t, rain);
            if (d.valid < 0.5 || t < d.t_spawn || t >= d.t_end) {
                continue;
            }
            let eat = eat_info(d, t, rain);
            if (eat.x > 0.5) {
                continue;
            }
            let r_base = r_now(d, t);
            let r = min(sqrt(max(r_base * r_base + eat.y, 0.0)), max(d.r0 * 2.15, 0.52));
            let x = drop_x(d, d.y);
            let fall_k = clamp((d.y - d.y_spawn) / max(PANE_END - d.y_spawn, 0.08), 0.0, 1.0);
            let egg = sd_egg(vec2<f32>((px - x) * COLS, (d.y - uv.y) * Y_STRETCH), mix(0.0, -0.2, fall_k));
            m = max(m, smoothstep(r / 1.5, 0.0, egg));
            if (d.y > d.y_spawn + 0.012) {
                let tail = max(d.y_spawn, 0.0);
                let gate = smoothstep(tail - 0.02, tail + 0.02, uv.y) * smoothstep(d.y + 0.02, d.y - 0.02, uv.y);
                if (gate > 0.0) {
                    let prog = smoothstep(tail, d.y, uv.y);
                    let width = r * 0.75 * sqrt(max(prog, 1e-5)) / COLS;
                    trail = max(trail, smoothstep(width, 0.0, abs(px - x)) * gate * 0.5);
                }
            }
        }
    }
    return vec2<f32>(smoothstep(0.3, 1.0, m), trail);
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
