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

// Sunset channel bias: Helmer / dnlzro/horizon (MIT, https://www.shadertoy.com/view/slSXRW).
const SUNSET_BIAS_STRENGTH: f32 = 0.1;
const BLOB_POWER: f32 = 0.85;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Sun/season/time only; same on all three vertices, so interpolation is a uniform.
    @location(1) zenith: vec3<f32>,
    @location(2) mid: vec3<f32>,
    @location(3) horizon: vec3<f32>,
    @location(4) wash: vec2<f32>,
    @location(5) well: vec2<f32>,
    @location(6) cool: vec2<f32>,
    @location(7) mesh_mid: vec2<f32>,
    @location(8) well_r: vec2<f32>,
    @location(9) punch: f32,
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
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
    for (var i = 0; i < 3; i = i + 1) {
        v += a * noise2(x);
        x = x * 2.02;
        a *= 0.5;
    }
    return v;
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

struct MeshLayout {
    wash: vec2<f32>,
    well: vec2<f32>,
    cool: vec2<f32>,
    mid: vec2<f32>,
    well_r: vec2<f32>,
    punch: f32,
}

fn blob_w(uv: vec2<f32>, center: vec2<f32>, radius: vec2<f32>) -> f32 {
    let d = (uv - center) / radius;
    return exp(-pow(dot(d, d), BLOB_POWER));
}

// Winter well sits right, summer left; altitude is height; time is slow drift.
fn mesh_layout(alt_deg: f32, season: f32, t: f32) -> MeshLayout {
    let day = smoothstep(-12.0, 50.0, alt_deg);
    let noon = smoothstep(20.0, 65.0, alt_deg);
    let twilight = 1.0 - smoothstep(6.0, 18.0, abs(alt_deg));
    let s = season * 6.2831853;
    let cx = cos(s);
    let sy = sin(s);
    let well = clamp(
        vec2<f32>(
            0.5 + 0.20 * cx + 0.08 * sy + 0.08 * sin(t * 0.011 + s) + 0.07 * twilight * cx,
            mix(0.26, 0.56, day) + 0.08 * noon + 0.06 * cos(t * 0.009 + 1.7 + s)
                - 0.05 * twilight + 0.08 * sy,
        ),
        vec2<f32>(0.12, 0.16),
        vec2<f32>(0.88, 0.78),
    );
    let cool = clamp(
        vec2<f32>(1.0 - well.x + 0.04 * sin(s + 2.0), mix(0.62, 0.78, day) + 0.05 * cos(t * 0.007 + s)),
        vec2<f32>(0.12, 0.40),
        vec2<f32>(0.88, 0.92),
    );
    let mid = clamp(
        vec2<f32>(mix(well.x, 0.5, 0.35) + 0.12 * sin(s + 1.3), well.y + 0.18 + 0.05 * sin(t * 0.013)),
        vec2<f32>(0.15, 0.20),
        vec2<f32>(0.85, 0.85),
    );
    let wash = vec2<f32>(0.5 + 0.10 * cos(s + 0.8) + 0.04 * sin(t * 0.006), mix(0.55, 0.72, day));
    let well_r = vec2<f32>(mix(0.62, 0.88, twilight), mix(0.50, 0.72, twilight)) * mix(0.95, 1.08, day);
    return MeshLayout(wash, well, cool, mid, well_r, 0.34 + 0.28 * twilight + 0.16 * day);
}

fn toward_well(uv: vec2<f32>, well: vec2<f32>) -> f32 {
    return blob_w(uv, well, vec2<f32>(0.72, 0.58));
}

fn mesh_look(uv: vec2<f32>, stops: SkyStops, mesh: MeshLayout, alt_deg: f32) -> vec3<f32> {
    let w0 = blob_w(uv, mesh.wash, vec2<f32>(0.95, 0.82)) + 0.06;
    let w1 = blob_w(uv, mesh.well, mesh.well_r);
    let w2 = blob_w(uv, mesh.cool, vec2<f32>(0.88, 0.78));
    let w3 = blob_w(uv, mesh.mid, vec2<f32>(0.52, 0.46));
    let day = smoothstep(-12.0, 50.0, alt_deg);
    let twilight = 1.0 - smoothstep(6.0, 18.0, abs(alt_deg));
    let well_col = mix_oklab(stops.horizon, vec3<f32>(1.0), 0.10 * day);
    let cool_col = mix_oklab(stops.zenith, stops.mid, 0.08);
    let l0 = linear_to_oklab(srgb_to_linear(stops.zenith));
    let l1 = linear_to_oklab(srgb_to_linear(well_col));
    let l2 = linear_to_oklab(srgb_to_linear(cool_col));
    let l3 = linear_to_oklab(srgb_to_linear(stops.mid));
    let sum = w0 + w1 + w2 + w3;
    var col = linear_to_srgb(oklab_to_linear((l0 * w0 + l1 * w1 + l2 * w2 + l3 * w3) / max(sum, 1e-4)));
    col = mix_oklab(col, well_col, w1 * mesh.punch);
    return mix_oklab(col, sunset_bias(well_col), w1 * twilight * 0.35);
}

fn star_layer(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let gid = floor(uv);
    let gv = fract(uv) - 0.5;
    var col = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let offs = vec2<f32>(f32(x), f32(y));
            let cell = gid + offs;
            let n = hash21(cell);
            let size = fract(n * 13.51);
            if (size < 0.68) {
                continue;
            }
            let p = gv - offs - (hash22(cell) - 0.5);
            let d = max(length(p), 1e-4);
            let sx = smoothstep(0.032, 0.0, abs(p.x)) * smoothstep(0.40, 0.0, abs(p.y));
            let sy = smoothstep(0.032, 0.0, abs(p.y)) * smoothstep(0.40, 0.0, abs(p.x));
            let core = (0.016 / d + max(sx, sy) * smoothstep(0.94, 0.995, size) * 0.40)
                * smoothstep(0.46, 0.09, d);
            let temp = fract(n * 7.13);
            let tint = mix(
                mix(vec3<f32>(0.72, 0.84, 1.00), vec3<f32>(0.96, 0.97, 1.00), smoothstep(0.12, 0.52, temp)),
                vec3<f32>(1.00, 0.88, 0.68),
                smoothstep(0.78, 0.96, temp),
            );
            let tw = 0.80 + 0.20 * sin(t * (1.05 + n * 2.6) + n * 17.0);
            col += tint * core * (0.30 + 0.95 * (size - 0.68) / 0.32) * tw;
        }
    }
    return col;
}

fn stars(uv: vec2<f32>, night: f32) -> vec3<f32> {
    if (night <= 0.0) {
        return vec3<f32>(0.0);
    }
    let aspect = u.resolution.x / max(u.resolution.y, 1.0);
    let p = vec2<f32>((uv.x - 0.5) * aspect, uv.y);
    let horizon = smoothstep(0.04, 0.24, uv.y);
    var col = vec3<f32>(0.0);
    col += star_layer(p * 15.0, u.time);
    col += star_layer(p * 27.0 + vec2<f32>(19.7, 8.1), u.time * 1.07) * 0.58;
    col += star_layer(p * 43.0 + vec2<f32>(5.3, 23.9), u.time * 0.91) * 0.34;
    return col * night * horizon * 0.70;
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
    let over_w = smoothstep(0.18, 0.92, cover);
    let dim = mix(0.90, 0.76, day);
    let overcast = mix_oklab(look, vec3<f32>(luma3(look)), 0.12) * dim;
    var out = mix(col, overcast, over_w * 0.70);
    out = mix(out, out * vec3<f32>(0.82, 0.86, 0.94), rain_amt * 0.32);
    let snow_hi = tinted(look, stops, 0.14 + 0.36 * day);
    out = mix(out, mix(out, snow_hi, 0.36), snow_amt * 0.40);
    return out;
}

fn cloud_color(
    look: vec3<f32>,
    stops: SkyStops,
    sun_y: f32,
    uv: vec2<f32>,
    well: vec2<f32>,
    thick: f32,
) -> vec3<f32> {
    let day = smoothstep(-0.15, 0.28, sun_y);
    let cover = clamp(u.cloud_cover, 0.0, 1.0);
    let tw = toward_well(uv, well);
    let lit = mix_oklab(stops.horizon, vec3<f32>(1.0), 0.06 * day);
    let shade = mix_oklab(stops.zenith, look, 0.35) * mix(0.70, 0.88, day);
    let lit_w = clamp(
        mix(0.52, 0.22, cover) * (0.35 + 0.65 * tw) * (0.40 + 0.60 * day) + (thick - 0.5) * 0.32,
        0.0,
        1.0,
    );
    var col = mix_oklab(shade, lit, lit_w);
    let glow = tw * smoothstep(0.22, 0.0, sun_y) * smoothstep(-0.20, 0.04, sun_y);
    col = mix_oklab(col, mix_oklab(stops.horizon, stops.mid, 0.35), glow * (0.45 + 0.22 * (1.0 - cover)));
    return col;
}

fn cloud_uv(uv: vec2<f32>, height_bias: f32) -> vec2<f32> {
    let y = max(uv.y + height_bias, 0.0);
    let persp = 1.0 / (y + 0.24);
    return vec2<f32>((uv.x - 0.5) * persp * 1.20, y / (y + 0.24));
}

// x = density, y = thickness.
fn clouds(uv: vec2<f32>) -> vec2<f32> {
    let cover = clamp(u.cloud_cover, 0.0, 1.0);
    if (cover < 0.02) {
        return vec2<f32>(0.0);
    }
    let t = u.time * 0.010;
    let p0 = cloud_uv(uv, 0.0);
    let warp = fbm(p0 * 0.45 + vec2<f32>(t * 0.18, 0.04)) - 0.5;
    let w = vec2<f32>(warp * 0.38, warp * 0.06);
    let n1 = fbm(p0 * 0.62 + w + vec2<f32>(t * 0.90, t * 0.22));
    let n2 = fbm(cloud_uv(uv, 0.14) * 1.25 + w * 0.65 + vec2<f32>(t * 1.40, -t * 0.16));

    let threshold = mix(0.64, 0.20, cover);
    let softness = mix(0.22, 0.38, cover);
    let d1 = smoothstep(threshold, threshold + softness, n1);
    let d2 = smoothstep(threshold + 0.08, threshold + softness + 0.10, n2);
    var dens = d1 * mix(0.55, 0.95, cover) + d2 * mix(0.20, 0.45, cover);
    let sheet = smoothstep(0.55, 1.0, cover) * mix(0.15, 0.55, cover);
    dens = clamp(max(dens, sheet * (0.65 + 0.35 * n1)), 0.0, 1.0);

    let fade = mix(smoothstep(0.0, 0.06, uv.y), 1.0, smoothstep(0.6, 1.0, cover) * 0.35);
    dens *= fade;
    return vec2<f32>(dens, n1 * 0.62 + n2 * 0.38);
}

fn sheet_lightning(uv: vec2<f32>, dens: f32, mesh: MeshLayout) -> f32 {
    let cover = clamp(u.cloud_cover, 0.0, 1.0);
    if (u.thunder < 0.02 || cover < 0.12 || dens < 0.02) {
        return 0.0;
    }
    let aspect = u.resolution.x / max(u.resolution.y, 1.0);
    let h = hash22(vec2<f32>(u.thunder_seed, 3.1));
    var origin = mix(mesh.well, mix(mesh.mid, mesh.cool, step(0.5, h.y)), 0.30 + 0.45 * h.x);
    origin += (h - 0.5) * 0.16;
    let delta = (uv - origin) * vec2<f32>(aspect * 0.58, 1.0);
    let masses = max(
        blob_w(uv, mesh.well, mesh.well_r),
        max(
            blob_w(uv, mesh.mid, vec2<f32>(0.52, 0.46)),
            blob_w(uv, mesh.cool, vec2<f32>(0.88, 0.78)),
        ),
    );
    let rim = 4.0 * masses * (1.0 - masses);
    let in_cloud = smoothstep(0.02, 0.18, dens);
    let glow = exp(-dot(delta, delta) * 12.0) * (rim + dens * 0.35) * in_cloud;
    let pop = hash21(vec2<f32>(floor(u.time * 28.0), 4.2));
    let crackle = mix(1.0, 0.78 + 0.22 * pop, smoothstep(0.2, 1.0, u.thunder));
    return glow * u.thunder * crackle;
}

fn snow(uv: vec2<f32>) -> f32 {
    let amt = u.precip * u.precip_kind;
    if (amt <= 0.0) {
        return 0.0;
    }
    let aspect = u.resolution.x / max(u.resolution.y, 1.0);
    let st = vec2<f32>(uv.x * aspect, uv.y);
    var acc = 0.0;
    for (var i = 0; i < 5; i = i + 1) {
        let fi = f32(i);
        let near = mix(0.32, 1.0, 1.0 - fi / 4.0);
        let scale = mix(8.0, 26.0, fi / 4.0);
        let fall = mix(0.14, 0.52, near);
        let shear = mix(-0.16, 0.20, hash21(vec2<f32>(fi, 2.1)));
        let wind = mix(0.10, 0.34, near);
        var p = st * scale + vec2<f32>(fi * 5.1, fi * 2.3);
        p.y -= u.time * fall;
        p.x += p.y * shear + sin(p.y * 0.38 + u.time * (0.28 + fi * 0.11) + fi) * wind;
        let rnd = hash22(floor(p) + vec2<f32>(fi * 5.3, 1.9));
        let vis = step(rnd.x, mix(0.12, 0.60, amt)) * near;
        let q = fract(p) - 0.5 - (rnd - 0.5) * 0.28;
        let vel = normalize(vec2<f32>(shear * 0.45 + wind * 0.25, -1.0));
        let across = abs(q.x * vel.y - q.y * vel.x);
        let along = q.x * vel.x + q.y * vel.y;
        let streak = mix(1.7, 3.4, near);
        let rad = mix(0.035, 0.12, rnd.y) * mix(0.42, 1.0, near);
        let d = length(vec2<f32>(across, along / streak));
        let x = clamp(1.0 - d / max(rad, 1e-4), 0.0, 1.0);
        acc += x * x * vis * mix(0.42, 1.0, near);
    }
    return clamp(acc, 0.0, 1.6) * mix(0.50, 1.0, amt);
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let pos = p[vid];
    let sun = normalize(u.sun_dir);
    let alt_deg = degrees(asin(clamp(sun.y, -1.0, 1.0)));
    let stops = solar_stops(alt_deg, u.season);
    let mesh = mesh_layout(alt_deg, u.season, u.time);
    var out: VsOut;
    out.pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = vec2<f32>(pos.x * 0.5 + 0.5, 0.5 - pos.y * 0.5);
    out.zenith = stops.zenith;
    out.mid = stops.mid;
    out.horizon = stops.horizon;
    out.wash = mesh.wash;
    out.well = mesh.well;
    out.cool = mesh.cool;
    out.mesh_mid = mesh.mid;
    out.well_r = mesh.well_r;
    out.punch = mesh.punch;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let sky_uv = vec2<f32>(uv.x, 1.0 - uv.y);

    let sun = normalize(u.sun_dir);
    let alt_deg = degrees(asin(clamp(sun.y, -1.0, 1.0)));
    let stops = SkyStops(in.zenith, in.mid, in.horizon);
    let mesh = MeshLayout(in.wash, in.well, in.cool, in.mesh_mid, in.well_r, in.punch);
    let look = mesh_look(sky_uv, stops, mesh, alt_deg);

    var col = look;
    col = weather_grade(col, look, stops, sun.y);

    let night = smoothstep(-0.02, -0.30, sun.y);
    col += stars(sky_uv, night * (1.0 - u.cloud_cover * 0.85));

    let cld = clouds(sky_uv);
    let day = smoothstep(-0.15, 0.28, sun.y);
    let cloud_col = cloud_color(look, stops, sun.y, sky_uv, mesh.well, cld.y);
    let cover = clamp(u.cloud_cover, 0.0, 1.0);
    col = mix(col, cloud_col, cld.x * mix(0.80, 0.94, cover));
    let flash = sheet_lightning(sky_uv, cld.x, mesh) * cld.x;
    col += mix(cloud_col, vec3<f32>(0.80, 0.87, 1.0), mix(0.40, 0.18, day))
        * flash
        * mix(2.2, 1.4, day);

    let flake = tinted(look, stops, 0.12 + 0.45 * day);
    col += flake * snow(uv) * 0.85;

    let wisp = fbm(vec2<f32>(uv.x * 1.3, uv.y * 1.8) + vec2<f32>(u.time * 0.022, u.time * 0.014));
    let fog_amt = clamp(sky_fog_amt(uv, u.fog) * mix(0.48, 1.45, wisp), 0.0, 1.0);
    let milk = mix(vec3<f32>(0.18, 0.22, 0.32), vec3<f32>(0.78, 0.80, 0.82), day);
    let fog_col = mix(mix(look, stops.horizon, 0.48), milk, mix(0.26, 0.52, day));
    col = mix(col, fog_col, fog_amt);
    col += (hash21(in.pos.xy) - 0.5) * 0.004;
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(col, 1.0);
}
