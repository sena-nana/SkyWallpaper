struct Uniforms {
    sun_dir: vec3<f32>,
    time: f32,
    resolution: vec2<f32>,
    cloud_cover: f32,
    precip: f32,
    precip_kind: f32,
    fog: f32,
    thunder: f32,
    cam_pitch: f32,
    cam_yaw: f32,
    exposure: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// Atmosphere: Hillaire 2020 coefficients via Andrew Helmer "Production Sky
// Rendering" (https://www.shadertoy.com/view/slSXRW, MIT) as ported in
// dnlzro/horizon `src/gradient.ts` (MIT). 2D view rays; not the CSS 1D gradient.
const PI: f32 = 3.141592653589793;
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
        let h = length(pos) - GROUND_RADIUS;
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

    let origin_radius = length(ray_origin);
    let pointing_down = dot(ray_origin, rd) / origin_radius < 0.0;
    let start_height = origin_radius - GROUND_RADIUS;
    let start_up = ray_origin / origin_radius;
    let start_ray_cos = clamp(dot(start_up, rd), -1.0, 1.0);
    let start_ray_angle = acos(abs(start_ray_cos));
    let transmittance_camera_to_space = compute_transmittance(start_height, start_ray_angle);

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
        var transmittance_camera_to_sample: vec3<f32>;
        if (pointing_down) {
            transmittance_camera_to_sample = transmittance_to_space / max(transmittance_camera_to_space, vec3<f32>(1e-6));
        } else {
            transmittance_camera_to_sample = transmittance_camera_to_space / max(transmittance_to_space, vec3<f32>(1e-6));
        }
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

fn look_dir(uv: vec2<f32>) -> vec3<f32> {
    let aspect = u.resolution.x / max(u.resolution.y, 1.0);
    let fov = 1.15;
    let px = (uv.x * 2.0 - 1.0) * aspect * fov;
    let py = (uv.y * 2.0 - 1.0) * fov;
    let pitch = u.cam_pitch * PI / 180.0;
    let yaw = u.cam_yaw * PI / 180.0;
    // ENU: x east, y up, z north. Yaw from north clockwise.
    let forward = vec3<f32>(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch));
    let world_up = vec3<f32>(0.0, 1.0, 0.0);
    let right = normalize(cross(forward, world_up));
    let up = cross(right, forward);
    return normalize(forward + right * px + up * py);
}

fn stars(rd: vec3<f32>, night: f32) -> vec3<f32> {
    if (night <= 0.0) {
        return vec3<f32>(0.0);
    }
    let n = floor(rd * 140.0);
    let h = hash21(n.xy + n.z * 17.0);
    let spark = smoothstep(0.9972, 1.0, h);
    let twinkle = 0.7 + 0.3 * sin(u.time * (1.5 + h * 3.0) + h * 20.0);
    return vec3<f32>(spark * twinkle * night);
}

fn weather_grade(col: vec3<f32>, sun_y: f32) -> vec3<f32> {
    let cover = clamp(u.cloud_cover, 0.0, 1.0);
    let rain_amt = u.precip * (1.0 - u.precip_kind);
    let snow_amt = u.precip * u.precip_kind;
    let day = clamp(sun_y + 0.25, 0.0, 1.0);
    let lum = dot(col, vec3<f32>(0.2126, 0.7152, 0.0722));
    let overcast = vec3<f32>(0.42, 0.47, 0.54) * (0.35 + 0.65 * day) + vec3<f32>(lum * 0.15);
    var out = mix(col, overcast, cover * 0.48);
    out = mix(out, out * vec3<f32>(0.78, 0.84, 0.94), rain_amt * 0.4);
    let snow_col = mix(out, vec3<f32>(0.82, 0.88, 0.94) * (0.45 + 0.55 * day), 0.4);
    out = mix(out, snow_col, snow_amt * 0.45);
    return out;
}

fn clouds(rd: vec3<f32>) -> f32 {
    if (rd.y < 0.02) {
        return 0.0;
    }
    let t = u.time * 0.012;
    let p1 = rd.xz / max(rd.y, 0.08) * 0.35 + vec2<f32>(t, t * 0.35);
    let p2 = rd.xz / max(rd.y, 0.08) * 0.9 + vec2<f32>(t * 1.4, -t * 0.2);
    let n = fbm(p1) * 0.65 + fbm(p2) * 0.35;
    let cover = clamp(u.cloud_cover, 0.0, 1.0);
    let threshold = mix(0.72, 0.18, cover);
    return smoothstep(threshold, threshold + 0.28, n) * mix(0.15, 1.0, cover);
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
    var rd = look_dir(vec2<f32>(uv.x, 1.0 - uv.y));
    if (rd.y < -0.02) {
        rd = normalize(vec3<f32>(rd.x, abs(rd.y) * 0.15 + 0.001, rd.z));
    }

    let sun = normalize(u.sun_dir);
    var col = atmosphere(rd, sun);
    col *= HORIZON_EXPOSURE * u.exposure;
    col = sunset_bias(col);
    col = weather_grade(col, sun.y);

    let night = smoothstep(0.15, -0.12, sun.y);
    col += stars(rd, night * (1.0 - u.cloud_cover * 0.85));

    let cld = clouds(rd);
    let cloud_lit = mix(vec3<f32>(0.15, 0.17, 0.22), vec3<f32>(0.95, 0.93, 0.9), clamp(sun.y * 0.8 + 0.35, 0.0, 1.0));
    let sunset = pow(max(dot(rd, sun), 0.0), 8.0) * smoothstep(0.35, -0.05, sun.y) * smoothstep(-0.25, 0.05, sun.y);
    let cloud_col = mix(cloud_lit, vec3<f32>(1.0, 0.45, 0.2), sunset * 0.8);
    col = mix(col, cloud_col, cld * 0.88);
    col += cloud_col * cld * u.thunder * 1.8;

    col += vec3<f32>(0.75, 0.85, 1.0) * rain(uv) * 0.55;
    col += vec3<f32>(0.95, 0.97, 1.0) * snow(uv) * 0.85;
    col += vec3<f32>(0.85, 0.9, 1.0) * lightning_bolt(uv) * 2.4;

    let fog_amt = u.fog * (1.0 - rd.y * 0.7);
    col = mix(col, vec3<f32>(0.55, 0.58, 0.62) * (0.4 + 0.6 * clamp(sun.y + 0.2, 0.0, 1.0)), fog_amt);

    col += vec3<f32>(u.thunder * 0.12);
    col = aces(col);
    col = pow(max(col, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    return vec4<f32>(col, 1.0);
}
