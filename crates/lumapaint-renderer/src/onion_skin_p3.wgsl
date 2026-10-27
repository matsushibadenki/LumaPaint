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

fn encode_srgb(value: vec3<f32>) -> vec3<f32> {
    return select(1.055*pow(max(value,vec3(0.0)),vec3(1.0/2.4))-0.055,value*12.92,value<=vec3(0.0031308));
}
fn decode_srgb(value: vec3<f32>) -> vec3<f32> {
    return select(pow((value+0.055)/1.055,vec3(2.4)),value/12.92,value<=vec3(0.04045));
}
@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let point = in.uv * u.document.xy + u.document.zw;
    if any(point < vec2(0.0)) || any(point >= u.document.xy) { discard; }
    let sampled=textureSample(svg_texture,svg_sampler,in.uv);
    if sampled.a<=0.00001 { return vec4(0.0); }
    // Cache rasters are premultiplied sRGB. Recover straight sRGB before
    // converting to the linear premultiplied color required by GPU blending.
    let straight=clamp(encode_srgb(sampled.rgb)/sampled.a,vec3(0.0),vec3(1.0));
    let rgb=decode_srgb(straight)*sampled.a;
    return vec4(0.82259287*rgb.r+0.17753395*rgb.g, 0.03319951*rgb.r+0.96678350*rgb.g, 0.01708535*rgb.r+0.07239572*rgb.g+0.91030148*rgb.b, sampled.a);
}
