struct Uniforms { viewport: vec4<f32>, appearance: vec4<f32>, document: vec4<f32> }
@group(0) @binding(0) var<uniform> u: Uniforms;
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) point: vec2<f32>,
    @location(1) @interpolate(flat) ends: vec4<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) radius: f32,
    @location(4) @interpolate(flat) hardness: f32,
    @location(5) @interpolate(flat) weight: f32,
    @location(6) @interpolate(flat) texture: f32,
}
@vertex fn vs_main(@builtin(vertex_index) vertex: u32, @location(0) ends: vec4<f32>, @location(1) color: vec4<f32>, @location(2) radius: f32, @location(3) hardness: f32, @location(4) weight: f32, @location(5) texture: f32) -> VertexOut {
    let corners = array<vec2<f32>, 6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,0.0),vec2(1.0,1.0));
    let size = u.viewport.xy / u.viewport.z;
    let scale = max(0.01, min((size.x-48.0)/u.document.x, (size.y-48.0)/u.document.y)) * u.viewport.w;
    let extra = radius + 2.0;
    let point = mix(min(ends.xy,ends.zw)-vec2(extra), max(ends.xy,ends.zw)+vec2(extra), corners[vertex]);
    let screen = (point-u.document.xy*0.5)*scale + size*0.5 + u.appearance.yz;
    var out: VertexOut;
    out.position = vec4(screen.x/size.x*2.0-1.0, 1.0-screen.y/size.y*2.0, 0.0, 1.0);
    out.point=point; out.ends=ends; out.color=color; out.radius=radius; out.hardness=hardness; out.weight=weight;
    out.texture=texture;
    return out;
}
@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let delta=in.ends.zw-in.ends.xy;
    let t=clamp(dot(in.point-in.ends.xy,delta)/max(dot(delta,delta),0.0001),0.0,1.0);
    let radial=in.point-(in.ends.xy+t*delta);
    let distance=length(radial);
    // Match the raster brush profile in document pixels. Screen derivatives
    // inflate the brush when zoomed out and shrink it on Retina/zoomed-in views.
    let aa=max((abs(radial.x)+abs(radial.y))/max(distance,0.001),0.01);
    let inner=in.radius*clamp(in.hardness,0.0,1.0);
    let profile=1.0-smoothstep(inner,max(inner+aa,in.radius+aa),distance);
    let density=mix(4.0,16.0,in.hardness);
    var grain_factor = 1.0;
    if in.texture > 0.0 {
        let cell = vec2<u32>(floor(max(in.point, vec2(0.0))));
        var hash = (cell.x * 1973u) ^ (cell.y * 9277u) ^ 89173u;
        hash = (hash ^ (hash >> 13u)) * 1274126177u;
        let grain = f32(hash & 255u) / 255.0;
        let pencil = 0.03 + 0.4 * grain * grain;
        grain_factor = mix(1.0, pencil, min(in.texture, 1.0));
        if in.texture > 1.0 {
            let dry_cell = vec2<u32>(floor(max(in.point, vec2(0.0)) / vec2(1.5, 5.0)));
            var dry_hash = (dry_cell.x * 1973u) ^ (dry_cell.y * 9277u) ^ 89173u;
            dry_hash = (dry_hash ^ (dry_hash >> 13u)) * 1274126177u;
            let dry = select(0.65, 0.0, f32(dry_hash & 255u) / 255.0 < 0.48);
            grain_factor = mix(pencil, dry, in.texture - 1.0);
        }
    }
    let optical_depth=density * in.weight * profile * grain_factor;
    if any(in.point<vec2(0.0)) || any(in.point>=u.document.xy) { discard; }
    if !selection_contains(in.point) { discard; }
    // Color is constant within this stroke. Store it once; only density adds.
    return vec4(in.color.rgb, optical_depth);
}
