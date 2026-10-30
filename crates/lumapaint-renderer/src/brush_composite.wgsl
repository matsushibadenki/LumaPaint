struct Uniforms {viewport:vec4<f32>,appearance:vec4<f32>,document:vec4<f32>}
@group(2) @binding(0) var<uniform> u:Uniforms;
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
    let size=u.viewport.xy/u.viewport.z;
    let pixel=in.position.xy/u.viewport.z;
    let scale=max(0.01,min((size.x-48.)/u.document.x,(size.y-48.)/u.document.y))*u.viewport.w;
    let point=(pixel-size*0.5-u.appearance.yz)/scale+u.document.xy*0.5;
    let coverage = (1.0 - exp(-accumulated.a))*selection_coverage(point);
    // Match the committed 8-bit tile only when screen samples coincide with
    // document pixel centers. At other zooms/pans retain continuous coverage
    // so magnified and subpixel previews keep smooth edges.
    let centered = all(abs(fract(point)-vec2(0.5)) < vec2(0.001));
    let aligned = abs(scale-1.0)<0.0001 && centered;
    let alpha = select(coverage, floor(coverage*255.0+0.5)/255.0, aligned);
    return vec4(accumulated.rgb * alpha, alpha);
}
