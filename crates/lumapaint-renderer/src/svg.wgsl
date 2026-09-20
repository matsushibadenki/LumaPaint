struct Uniforms { viewport: vec4<f32>, appearance: vec4<f32> }
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
    let scale = max(0.01, min((size.x-48.0)/960.0, (size.y-48.0)/640.0)) * u.viewport.w;
    let point = corners[index] * vec2(960.0, 640.0);
    let screen = (point-vec2(480.0,320.0))*scale + size*0.5;
    var out: VertexOut;
    out.position = vec4(screen.x/size.x*2.0-1.0, 1.0-screen.y/size.y*2.0, 0.0, 1.0);
    out.uv = corners[index];
    return out;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(svg_texture, svg_sampler, in.uv);
}
