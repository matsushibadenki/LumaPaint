@group(0) @binding(0) var brush_texture: texture_2d<f32>;
@group(0) @binding(1) var brush_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0)
    );
    let uv = corners[index];
    var out: VertexOut;
    out.position = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let accumulated = textureSample(brush_texture, brush_sampler, in.uv);
    let alpha = 1.0 - exp(-accumulated.a);
    return vec4(accumulated.rgb * alpha, alpha);
}
