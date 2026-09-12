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

// Atmosphere: Hillaire 2020 via Helmer (MIT, https://www.shadertoy.com/view/slSXRW)
// as ported in dnlzro/horizon `src/gradient.ts` (MIT). Display stops mix in OKLab;
// physical scattering is twilight Mie only.
const PI: f32 = 3.141592653589793;
const FOV_DEG: f32 = 75.0;
const RAYLEIGH_SCATTER: vec3<f32> = vec3<f32>(5.802e-6, 13.558e-6, 33.1e-6);
const MIE_SCATTER: f32 = 3.996e-6;
const MIE_ABSORB: f32 = 4.44e-6;
const OZONE_ABSORB: vec3<f32> = vec3<f32>(0.65e-6, 1.881e-6, 0.085e-6);
const RAYLEIGH_SCALE_HEIGHT: f32 = 8e3;
const MIE_SCALE_HEIGHT: f32 = 1.2e3;
const GROUND_RADIUS: f32 = 6360e3;
const TOP_RADIUS: f32 = 6460e3;
const HORIZON_EXPOSURE: f32 = 25.0;
const SUNSET_BIAS_STRENGTH: f32 = 0.1;
const MIE_G: f32 = 0.8;
const VIEW_STEPS: i32 = 16;
const T_STEPS: i32 = 12;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(p[vid], 0.0, 1.0);
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var x = p;
    for (var i = 0; i < 5; i = i + 1) {
        v += a * noise2(x);
        x = x * 2.02;
        a *= 0.5;
    }
    return v;
}

fn rayleigh_phase(angle: f32) -> f32 {
    let c = cos(angle);
    return 3.0 * (1.0 + c * c) / (16.0 * PI);
}

fn mie_phase(angle: f32) -> f32 {
    let g = MIE_G;
    let c = cos(angle);
    let scale = 3.0 / (8.0 * PI);
    let num = (1.0 - g * g) * (1.0 + c * c);
    let denom = (2.0 + g * g) * pow(1.0 + g * g - 2.0 * g * c, 1.5);
    return (scale * num) / max(denom, 1e-8);
}

// First positive hit with a sphere at the origin (Real-Time Collision Detection 5.3.2).
fn intersect_sphere(p: vec3<f32>, d: vec3<f32>, radius: f32) -> f32 {
    let b = dot(p, d);
    let c = dot(p, p) - radius * radius;
    let discr = b * b - c;
    if (discr < 0.0) {
        return -1.0;
    }
    let s = sqrt(discr);
    let t_near = -b - s;
    if (t_near < 0.0) {
        return -b + s;
    }
    return t_near;
}

fn compute_transmittance(height: f32, angle: f32) -> vec3<f32> {
    let ray_origin = vec3<f32>(0.0, GROUND_RADIUS + height, 0.0);
    let ray_direction = vec3<f32>(sin(angle), cos(angle), 0.0);
    let t_ground = intersect_sphere(ray_origin, ray_direction, GROUND_RADIUS);
    if (t_ground > 1e-3 || (t_ground >= 0.0 && ray_direction.y < 0.0)) {
        return vec3<f32>(0.0);
    }
    let distance = intersect_sphere(ray_origin, ray_direction, TOP_RADIUS);
    if (distance <= 0.0) {
        return vec3<f32>(1.0);
    }
    let segment = distance / f32(T_STEPS);
    var t = 0.5 * segment;
    var od_rayleigh = 0.0;
    var od_mie = 0.0;
    var od_ozone = 0.0;
    for (var i = 0; i < T_STEPS; i = i + 1) {
        let pos = ray_origin + ray_direction * t;
        let h = max(length(pos) - GROUND_RADIUS, 0.0);
        od_rayleigh += exp(-h / RAYLEIGH_SCALE_HEIGHT) * segment;
        od_mie += exp(-h / MIE_SCALE_HEIGHT) * segment;
        let ozone_density = 1.0 - min(abs(h - 25e3) / 15e3, 1.0);
        od_ozone += ozone_density * segment;
        t += segment;
    }
    let tau = RAYLEIGH_SCATTER * od_rayleigh + vec3<f32>(MIE_ABSORB) * od_mie + OZONE_ABSORB * od_ozone;
    return exp(-tau);
}

fn atmosphere(rd: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    let ray_origin = vec3<f32>(0.0, GROUND_RADIUS, 0.0);
    let t_exit = intersect_sphere(ray_origin, rd, TOP_RADIUS);
    if (t_exit <= 0.0) {
        return vec3<f32>(0.0);
    }

    let segment = t_exit / f32(VIEW_STEPS);
    var t_ray = segment * 0.5;
    var inscattered = vec3<f32>(0.0);

    let start_ray_angle = acos(abs(clamp(rd.y, -1.0, 1.0)));
    let transmittance_camera_to_space = compute_transmittance(0.0, start_ray_angle);

    for (var i = 0; i < VIEW_STEPS; i = i + 1) {
        let sample_pos = ray_origin + rd * t_ray;
        let sample_radius = length(sample_pos);
        let up_unit = sample_pos / max(sample_radius, 1.0);
        let sample_height = sample_radius - GROUND_RADIUS;

        let view_cos = clamp(dot(up_unit, rd), -1.0, 1.0);
        let sun_cos = clamp(dot(up_unit, sun), -1.0, 1.0);
        let view_angle = acos(abs(view_cos));
        let sun_angle = acos(sun_cos);

        let transmittance_to_space = compute_transmittance(sample_height, view_angle);
        let transmittance_camera_to_sample =
            transmittance_camera_to_space / max(transmittance_to_space, vec3<f32>(1e-6));
        let transmittance_light = compute_transmittance(sample_height, sun_angle);

        let density_r = exp(-sample_height / RAYLEIGH_SCALE_HEIGHT);
        let density_m = exp(-sample_height / MIE_SCALE_HEIGHT);
        let sun_view_cos = clamp(dot(sun, rd), -1.0, 1.0);
        let sun_view_angle = acos(sun_view_cos);
        let phase_r = rayleigh_phase(sun_view_angle);
        let phase_m = mie_phase(sun_view_angle);
        let scattered = transmittance_light * (RAYLEIGH_SCATTER * density_r * phase_r + MIE_SCATTER * density_m * phase_m);
        inscattered += transmittance_camera_to_sample * scattered * segment;
        t_ray += segment;
    }
    return inscattered;
}

fn sunset_bias(color: vec3<f32>) -> vec3<f32> {
    let lum = 0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b;
    let w = 1.0 / (1.0 + 2.0 * lum);
    let k = SUNSET_BIAS_STRENGTH;
    let rb = 1.0 + 0.5 * k * w;
    let gb = 1.0 - 0.5 * k * w;
    let bb = 1.0 + 1.0 * k * w;
    return max(color * vec3<f32>(rb, gb, bb), vec3<f32>(0.0));
}

// Horizon view ray: s=0 at the horizon (bottom), s=1 toward mid-sky (top).
fn view_dir(s: f32) -> vec3<f32> {
    let focal_z = 1.0 / tan(FOV_DEG * 0.5 * PI / 180.0);
    return normalize(vec3<f32>(0.0, s, focal_z));
}

fn cbrt(x: f32) -> f32 {
    return sign(x) * pow(abs(x), 1.0 / 3.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}

fn linear_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
    let m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
    let s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
    let l_ = cbrt(l);
    let m_ = cbrt(m);
    let s_ = cbrt(s);
    return vec3<f32>(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    );
}

fn oklab_to_linear(lab: vec3<f32>) -> vec3<f32> {
    let l_ = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m_ = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s_ = lab.x - 0.0897335040 * lab.y - 1.2914855480 * lab.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    return vec3<f32>(
         4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

fn mix_oklab(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    let w = clamp(t, 0.0, 1.0);
    let la = linear_to_oklab(srgb_to_linear(a));
    let lb = linear_to_oklab(srgb_to_linear(b));
    return linear_to_srgb(oklab_to_linear(mix(la, lb, w)));
}

struct SkyStops {
    zenith: vec3<f32>,
    mid: vec3<f32>,
    horizon: vec3<f32>,
}

fn mix_stops(a: SkyStops, b: SkyStops, t: f32) -> SkyStops {
    return SkyStops(
        mix_oklab(a.zenith, b.zenith, t),
        mix_oklab(a.mid, b.mid, t),
        mix_oklab(a.horizon, b.horizon, t),
    );
}

// 6 phases (night, blue hour, twilight, golden, day, noon) × 4 seasons
// (winter, spring, summer, autumn).
const LOOK_Z: array<vec3<f32>, 24> = array<vec3<f32>, 24>(
    vec3<f32>(0.102, 0.141, 0.220), vec3<f32>(0.110, 0.133, 0.255), vec3<f32>(0.110, 0.141, 0.282), vec3<f32>(0.118, 0.137, 0.235),
    vec3<f32>(0.094, 0.141, 0.282), vec3<f32>(0.110, 0.145, 0.333), vec3<f32>(0.102, 0.141, 0.345), vec3<f32>(0.118, 0.137, 0.298),
    vec3<f32>(0.102, 0.157, 0.376), vec3<f32>(0.141, 0.188, 0.439), vec3<f32>(0.110, 0.141, 0.408), vec3<f32>(0.141, 0.118, 0.314),
    vec3<f32>(0.227, 0.314, 0.502), vec3<f32>(0.188, 0.365, 0.596), vec3<f32>(0.165, 0.353, 0.604), vec3<f32>(0.216, 0.298, 0.486),
    vec3<f32>(0.353, 0.541, 0.671), vec3<f32>(0.282, 0.580, 0.800), vec3<f32>(0.239, 0.561, 0.831), vec3<f32>(0.345, 0.533, 0.722),
    vec3<f32>(0.541, 0.686, 0.784), vec3<f32>(0.384, 0.722, 0.878), vec3<f32>(0.431, 0.706, 0.910), vec3<f32>(0.416, 0.659, 0.831),
);
const LOOK_M: array<vec3<f32>, 24> = array<vec3<f32>, 24>(
    vec3<f32>(0.141, 0.188, 0.282), vec3<f32>(0.157, 0.180, 0.325), vec3<f32>(0.157, 0.188, 0.337), vec3<f32>(0.165, 0.176, 0.298),
    vec3<f32>(0.227, 0.282, 0.471), vec3<f32>(0.290, 0.259, 0.541), vec3<f32>(0.290, 0.282, 0.565), vec3<f32>(0.275, 0.247, 0.463),
    vec3<f32>(0.690, 0.439, 0.565), vec3<f32>(0.878, 0.565, 0.627), vec3<f32>(0.769, 0.353, 0.471), vec3<f32>(0.816, 0.439, 0.314),
    vec3<f32>(0.910, 0.690, 0.659), vec3<f32>(0.941, 0.690, 0.580), vec3<f32>(0.910, 0.627, 0.471), vec3<f32>(0.925, 0.655, 0.439),
    vec3<f32>(0.541, 0.678, 0.769), vec3<f32>(0.510, 0.753, 0.878), vec3<f32>(0.455, 0.722, 0.910), vec3<f32>(0.580, 0.698, 0.800),
    vec3<f32>(0.710, 0.804, 0.878), vec3<f32>(0.596, 0.831, 0.933), vec3<f32>(0.624, 0.816, 0.949), vec3<f32>(0.659, 0.784, 0.878),
);
const LOOK_H: array<vec3<f32>, 24> = array<vec3<f32>, 24>(
    vec3<f32>(0.220, 0.282, 0.376), vec3<f32>(0.235, 0.267, 0.416), vec3<f32>(0.227, 0.251, 0.408), vec3<f32>(0.247, 0.263, 0.369),
    vec3<f32>(0.345, 0.408, 0.565), vec3<f32>(0.431, 0.353, 0.612), vec3<f32>(0.416, 0.353, 0.627), vec3<f32>(0.400, 0.337, 0.510),
    vec3<f32>(0.910, 0.627, 0.565), vec3<f32>(0.941, 0.690, 0.565), vec3<f32>(0.910, 0.471, 0.282), vec3<f32>(0.910, 0.565, 0.251),
    vec3<f32>(0.941, 0.784, 0.690), vec3<f32>(0.961, 0.800, 0.627), vec3<f32>(0.941, 0.753, 0.439), vec3<f32>(0.957, 0.769, 0.408),
    vec3<f32>(0.769, 0.831, 0.878), vec3<f32>(0.753, 0.890, 0.941), vec3<f32>(0.706, 0.863, 0.957), vec3<f32>(0.831, 0.847, 0.878),
    vec3<f32>(0.847, 0.894, 0.933), vec3<f32>(0.816, 0.933, 0.965), vec3<f32>(0.824, 0.922, 0.973), vec3<f32>(0.886, 0.890, 0.910),
);

fn mix4(season: f32, phase: i32, which: i32) -> vec3<f32> {
    let base = u32(clamp(phase, 0, 5)) * 4u;
    let x = fract(season) * 4.0;
    let i = base + (u32(floor(x)) % 4u);
    let j = base + ((u32(floor(x)) + 1u) % 4u);
    var a = LOOK_Z[i];
    var b = LOOK_Z[j];
    switch which {
        case 1: { a = LOOK_M[i]; b = LOOK_M[j]; }
        case 2: { a = LOOK_H[i]; b = LOOK_H[j]; }
        default: {}
    }
    return mix_oklab(a, b, fract(x));
}

fn phase_palette(phase: i32, season: f32) -> SkyStops {
    return SkyStops(mix4(season, phase, 0), mix4(season, phase, 1), mix4(season, phase, 2));
}

fn solar_stops(alt_deg: f32, season: f32) -> SkyStops {
    var s = phase_palette(0, season);
    let keys = array<vec2<f32>, 5>(
        vec2<f32>(-18.0, -4.0),
        vec2<f32>(-4.0, 2.0),
        vec2<f32>(2.0, 10.0),
        vec2<f32>(10.0, 45.0),
        vec2<f32>(45.0, 70.0),
    );
    for (var i = 0; i < 5; i = i + 1) {
        s = mix_stops(s, phase_palette(i + 1, season), smoothstep(keys[i].x, keys[i].y, alt_deg));
    }
    return s;
}

fn solar_look(alt_deg: f32, season: f32, rd_y: f32) -> vec3<f32> {
    let stops = solar_stops(alt_deg, season);
    let t = pow(clamp(rd_y / 0.55, 0.0, 1.0), 0.62);
    var col = mix_oklab(stops.horizon, stops.zenith, t);
    let mid_w = smoothstep(0.0, 0.10, rd_y) * (1.0 - smoothstep(0.20, 0.46, rd_y));
    col = mix_oklab(col, stops.mid, mid_w * 0.70);
    return col;
}

fn stars(uv: vec2<f32>, night: f32) -> vec3<f32> {
    if (night <= 0.0) {
        return vec3<f32>(0.0);
    }
    let n = floor(uv * vec2<f32>(96.0, 54.0));
    let h = hash21(n);
    let spark = smoothstep(0.9965, 1.0, h);
    let twinkle = 0.7 + 0.3 * sin(u.time * (1.5 + h * 3.0) + h * 20.0);
    return vec3<f32>(0.95, 0.96, 1.0) * (spark * twinkle * night * 0.55);
}

fn luma3(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn tinted(look: vec3<f32>, stops: SkyStops, toward_white: f32) -> vec3<f32> {
    let base = mix(look, stops.horizon, 0.55);
    let soft = mix(base, vec3<f32>(luma3(base)), 0.12);
    return mix(soft, vec3<f32>(0.96, 0.97, 0.99), clamp(toward_white, 0.0, 1.0));
}

fn weather_grade(col: vec3<f32>, look: vec3<f32>, stops: SkyStops, sun_y: f32) -> vec3<f32> {
    let cover = clamp(u.cloud_cover, 0.0, 1.0);
    let rain_amt = u.precip * (1.0 - u.precip_kind);
    let snow_amt = u.precip * u.precip_kind;
    let day = smoothstep(-0.12, 0.28, sun_y);
    let overcast = mix_oklab(look, vec3<f32>(luma3(look)), 0.16)
        * mix(vec3<f32>(0.90, 0.91, 0.96), vec3<f32>(0.88, 0.90, 0.94), day);
    var out = mix(col, overcast, cover * 0.40);
    out = mix(out, out * vec3<f32>(0.82, 0.86, 0.94), rain_amt * 0.32);
    let snow_hi = tinted(look, stops, 0.14 + 0.36 * day);
    out = mix(out, mix(out, snow_hi, 0.36), snow_amt * 0.40);
    return out;
}

fn cloud_color(look: vec3<f32>, stops: SkyStops, sun_y: f32, rd_y: f32) -> vec3<f32> {
    let day = smoothstep(-0.15, 0.28, sun_y);
    let lit = mix_oklab(stops.horizon, vec3<f32>(1.0), 0.08 * day);
    let shade = mix_oklab(stops.zenith, look, 0.4) * mix(0.78, 0.92, day);
    var col = mix_oklab(shade, lit, 0.35 + 0.50 * day);
    let glow = (1.0 - rd_y) * smoothstep(0.22, 0.0, sun_y) * smoothstep(-0.20, 0.04, sun_y);
    col = mix_oklab(col, mix_oklab(stops.horizon, stops.mid, 0.35), glow * 0.55);
    return col;
}

fn clouds(uv: vec2<f32>) -> f32 {
    let t = u.time * 0.012;
    let y = max(uv.y, 0.08);
    let p1 = vec2<f32>(uv.x * 2.0, 1.0 / y) * 0.35 + vec2<f32>(t, t * 0.35);
    let p2 = vec2<f32>(uv.x * 2.0, 1.0 / y) * 0.9 + vec2<f32>(t * 1.4, -t * 0.2);
    let n = fbm(p1) * 0.65 + fbm(p2) * 0.35;
    let cover = clamp(u.cloud_cover, 0.0, 1.0);
    let threshold = mix(0.72, 0.18, cover);
    let fade = smoothstep(0.0, 0.08, uv.y);
    return smoothstep(threshold, threshold + 0.28, n) * mix(0.15, 1.0, cover) * fade;
}

fn rain(uv: vec2<f32>) -> f32 {
    let t = u.time * 2.8;
    let st = vec2<f32>(uv.x * 42.0, uv.y * 18.0 - t);
    let id = floor(st);
    let f = fract(st);
    let h = hash21(id);
    let sx = fract(h * 13.7);
    let drop = smoothstep(0.07, 0.0, abs(f.x - sx)) * smoothstep(0.0, 0.15, f.y) * smoothstep(1.0, 0.55, f.y);
    return drop * u.precip * (1.0 - u.precip_kind);
}

fn snow(uv: vec2<f32>) -> f32 {
    let t = u.time * 0.35;
    var acc = 0.0;
    for (var i = 0; i < 3; i = i + 1) {
        let fi = f32(i);
        let st = uv * (18.0 + fi * 9.0) + vec2<f32>(fi * 5.1, -t * (0.8 + fi * 0.4));
        let id = floor(st);
        let f = fract(st);
        let h = hash21(id + fi * 3.0);
        let d = length(f - vec2<f32>(fract(h * 7.1), fract(h * 3.3)));
        acc += smoothstep(0.06, 0.0, d);
    }
    return acc * u.precip * u.precip_kind;
}

fn lightning_bolt(uv: vec2<f32>) -> f32 {
    if (u.thunder < 0.2) {
        return 0.0;
    }
    let seed = fract(u.thunder * 7.13 + 0.17);
    var x = 0.35 + seed * 0.3;
    var y = 0.95;
    var acc = 0.0;
    for (var i = 0; i < 10; i = i + 1) {
        let ny = y - 0.08;
        let nx = x + (hash21(vec2<f32>(f32(i), seed * 20.0)) - 0.5) * 0.12;
        let pa = vec2<f32>(x, y);
        let pb = vec2<f32>(nx, ny);
        let ba = pb - pa;
        let h = clamp(dot(uv - pa, ba) / max(dot(ba, ba), 1e-4), 0.0, 1.0);
        let d = length(uv - pa - ba * h);
        acc += smoothstep(0.012, 0.0, d);
        x = nx;
        y = ny;
    }
    return acc * u.thunder;
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(@builtin(position) clip: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = clip.xy / u.resolution;
    let sky_uv = vec2<f32>(uv.x, 1.0 - uv.y);
    let rd = view_dir(sky_uv.y);

    let sun = normalize(u.sun_dir);
    let alt_deg = degrees(asin(clamp(sun.y, -1.0, 1.0)));
    let look = solar_look(alt_deg, u.season, rd.y);
    let w_look = mix(1.0, 0.90, smoothstep(-8.0, 55.0, alt_deg));
    let stops = solar_stops(alt_deg, u.season);

    var phys = atmosphere(rd, sun);
    phys *= HORIZON_EXPOSURE;
    phys = sunset_bias(phys);
    phys = aces(phys);
    phys = pow(max(phys, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));

    var col = mix(phys, look, w_look);
    let mie_w = smoothstep(14.0, 2.0, abs(alt_deg))
        * pow(1.0 - clamp(rd.y / 0.45, 0.0, 1.0), 1.35);
    col = mix(col, mix(col, phys, 0.40), mie_w);
    col = weather_grade(col, look, stops, sun.y);

    let night = smoothstep(0.02, -0.22, sun.y);
    col += stars(sky_uv, night * (1.0 - u.cloud_cover * 0.85));

    let cld = clouds(sky_uv);
    let day = smoothstep(-0.15, 0.28, sun.y);
    let cloud_col = cloud_color(look, stops, sun.y, rd.y);
    col = mix(col, cloud_col, cld * 0.88);
    col += cloud_col * cld * u.thunder * 1.8;

    col += vec3<f32>(0.75, 0.85, 1.0) * rain(uv) * 0.55;
    let flake = tinted(look, stops, 0.12 + 0.45 * day);
    col += flake * snow(uv) * 0.85;
    col += vec3<f32>(0.85, 0.9, 1.0) * lightning_bolt(uv) * 2.4;

    let fog_amt = u.fog * (1.0 - rd.y * 0.7);
    let fog_col = mix(mix(look, stops.horizon, 0.4), vec3<f32>(luma3(look)), 0.16)
        * mix(0.92, 1.06, day);
    col = mix(col, fog_col, fog_amt);

    col += vec3<f32>(u.thunder * 0.12);
    col += (hash21(clip.xy) - 0.5) * 0.004;
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(col, 1.0);
}
