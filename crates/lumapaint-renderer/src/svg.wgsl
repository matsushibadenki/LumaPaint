struct Uniforms { viewport: vec4<f32>, appearance: vec4<f32>, document: vec4<f32> }
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var svg_texture: texture_2d<f32>;
@group(1) @binding(1) var svg_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let corners = array<vec2<f32>, 6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,0.0),vec2(1.0,1.0));
    let size = u.viewport.xy / u.viewport.z;
    let scale = max(0.01, min((size.x-48.0)/u.document.x, (size.y-48.0)/u.document.y)) * u.viewport.w;
    let point = corners[index] * u.document.xy + u.document.zw;
    let screen = (point-u.document.xy*0.5)*scale + size*0.5 + u.appearance.yz;
    var out: VertexOut;
    out.position = vec4(screen.x/size.x*2.0-1.0, 1.0-screen.y/size.y*2.0, 0.0, 1.0);
    out.uv = corners[index];
    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let point = in.uv * u.document.xy + u.document.zw;
    if any(point < vec2(0.0)) || any(point >= u.document.xy) { discard; }
    return textureSample(svg_texture, svg_sampler, in.uv);
}
