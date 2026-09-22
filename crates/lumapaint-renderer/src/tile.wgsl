struct Uniforms { viewport: vec4<f32>, appearance: vec4<f32>, document: vec4<f32> }
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tile_texture: texture_2d<f32>;
@group(1) @binding(1) var tile_sampler: sampler;

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
    let encoded = textureSample(tile_texture, tile_sampler, in.uv);
    if encoded.a <= 0.0 { return vec4(0.0); }
    // Tiles contain sRGB-encoded RGB premultiplied by alpha. Decode the
    // straight color before blending in the GPU's linear color space.
    let straight = clamp(encoded.rgb / encoded.a, vec3(0.0), vec3(1.0));
    let linear = select(straight / 12.92, pow((straight + 0.055) / 1.055, vec3(2.4)), straight > vec3(0.04045));
    return vec4(linear * encoded.a, encoded.a);
}
