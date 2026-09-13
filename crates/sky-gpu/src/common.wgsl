fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn hash22(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(hash21(p), hash21(p + vec2<f32>(19.2, 8.4)));
}

fn view_dir(s: f32) -> vec3<f32> {
    // s=0 at the horizon, s=1 toward mid-sky.
    let focal_z = 1.0 / tan(75.0 * 0.5 * 3.141592653589793 / 180.0);
    return normalize(vec3<f32>(0.0, s, focal_z));
}

fn sky_fog_amt(uv: vec2<f32>, fog: f32) -> f32 {
    let toward_horizon = 1.0 - view_dir(1.0 - uv.y).y;
    let depth = smoothstep(0.35, 0.95, toward_horizon);
    return 1.0 - exp(-fog * depth * 3.2);
}
