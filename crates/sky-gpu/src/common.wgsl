fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn view_dir(s: f32) -> vec3<f32> {
    // s=0 at the horizon, s=1 toward mid-sky.
    let focal_z = 1.0 / tan(75.0 * 0.5 * 3.141592653589793 / 180.0);
    return normalize(vec3<f32>(0.0, s, focal_z));
}

fn sky_fog_amt(uv: vec2<f32>, fog: f32) -> f32 {
    return fog * (1.0 - view_dir(1.0 - uv.y).y * 0.7);
}

fn lightning_rgb(uv: vec2<f32>, thunder: f32) -> vec3<f32> {
    if (thunder < 0.2) {
        return vec3<f32>(0.0);
    }
    let seed = fract(thunder * 7.13 + 0.17);
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
    return vec3<f32>(0.85, 0.9, 1.0) * acc * thunder * 2.4;
}
