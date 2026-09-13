@group(0) @binding(1) var source_tex: texture_2d<f32>;
@group(0) @binding(2) var source_samp: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[vid], 0.0, 1.0);
    out.uv = vec2<f32>(
        positions[vid].x * 0.5 + 0.5,
        0.5 - positions[vid].y * 0.5,
    );
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(source_tex, source_samp, in.uv);
}
