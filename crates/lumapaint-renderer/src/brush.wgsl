struct Uniforms { viewport: vec4<f32>, appearance: vec4<f32> }
@group(0) @binding(0) var<uniform> u: Uniforms;
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) point: vec2<f32>,
    @location(1) @interpolate(flat) ends: vec4<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) radius: f32,
}
@vertex fn vs_main(@builtin(vertex_index) vertex: u32, @location(0) ends: vec4<f32>, @location(1) color: vec4<f32>, @location(2) radius: f32) -> VertexOut {
    let corners = array<vec2<f32>, 6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,0.0),vec2(1.0,1.0));
    let size = u.viewport.xy / u.viewport.z;
    let scale = max(0.01, min((size.x-48.0)/960.0, (size.y-48.0)/640.0)) * u.viewport.w;
    let extra = radius + 1.5 / scale;
    let point = mix(min(ends.xy,ends.zw)-vec2(extra), max(ends.xy,ends.zw)+vec2(extra), corners[vertex]);
    let screen = (point-vec2(480.0,320.0))*scale + size*0.5;
    var out: VertexOut;
    out.position = vec4(screen.x/size.x*2.0-1.0, 1.0-screen.y/size.y*2.0, 0.0, 1.0);
    out.point=point; out.ends=ends; out.color=color; out.radius=radius;
    return out;
}
@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let delta=in.ends.zw-in.ends.xy;
    let t=clamp(dot(in.point-in.ends.xy,delta)/max(dot(delta,delta),0.0001),0.0,1.0);
    let distance=length(in.point-(in.ends.xy+t*delta));
    let aa=max(fwidth(distance),0.01);
    let alpha=1.0-smoothstep(in.radius-aa,in.radius+aa,distance);
    if any(in.point<vec2(0.0)) || any(in.point>=vec2(960.0,640.0)) { discard; }
    return vec4(in.color.rgb,alpha);
}
