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
    @location(7) @interpolate(flat) material: vec4<f32>,
    @location(8) @interpolate(flat) medium_aux: vec4<f32>,
    @location(9) @interpolate(flat) paper: vec4<f32>,
    @location(10) @interpolate(flat) paper_transform: vec4<f32>,
    @location(11) @interpolate(flat) paper_offset: vec4<f32>,
    @location(12) @interpolate(flat) grain_transform: vec4<f32>,
    @location(13) @interpolate(flat) grain_offset: vec4<f32>,
}
@vertex fn vs_main(@builtin(vertex_index) vertex: u32, @location(0) ends: vec4<f32>, @location(1) color: vec4<f32>, @location(2) radius: f32, @location(3) hardness: f32, @location(4) weight: f32, @location(5) texture: f32, @location(6) material:vec4<f32>, @location(7) medium_aux:vec4<f32>, @location(8) paper:vec4<f32>, @location(9) paper_transform:vec4<f32>, @location(10) paper_offset:vec4<f32>, @location(11) grain_transform:vec4<f32>, @location(12) grain_offset:vec4<f32>) -> VertexOut {
    let corners = array<vec2<f32>, 6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,0.0),vec2(1.0,1.0));
    let size = u.viewport.xy / u.viewport.z;
    let scale = max(0.01, min((size.x-48.0)/u.document.x, (size.y-48.0)/u.document.y)) * u.viewport.w;
    let extra = radius + 2.0;
    let point = mix(min(ends.xy,ends.zw)-vec2(extra), max(ends.xy,ends.zw)+vec2(extra), corners[vertex]);
    let screen = (point-u.document.xy*0.5)*scale + size*0.5 + u.appearance.yz;
    var out: VertexOut;
    out.position = vec4(screen.x/size.x*2.0-1.0, 1.0-screen.y/size.y*2.0, 0.0, 1.0);
    out.point=point; out.ends=ends; out.color=color; out.radius=radius; out.hardness=hardness; out.weight=weight;
    out.texture=texture;out.material=material;out.medium_aux=medium_aux;out.paper=paper;out.paper_transform=paper_transform;out.paper_offset=paper_offset;out.grain_transform=grain_transform;out.grain_offset=grain_offset;
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
    var samples=1u;
    if in.texture>0. || in.material.x>0.5 { samples=media_samples(in.grain_transform); }
    if in.material.x>0.5 && in.paper.x>0.5 { samples=max(samples,media_samples(in.paper_transform)); }
    var footprint=1.;
    if in.texture>0. || in.material.x>0.5 { footprint=media_footprint(in.grain_transform); }
    if in.material.x>0.5 && in.paper.x>0.5 { footprint=max(footprint,media_footprint(in.paper_transform)); }
    let stable_point=floor(in.point*256.+vec2(0.5))/256.;
    var pigment=0.;
    for(var j=0u;j<samples;j++){for(var i=0u;i<samples;i++){
        let offset=media_sample_offset(i,j,samples,footprint>8.);
        let sample_point=stable_point+offset;
        let grain_raw=media_point(sample_point,in.grain_transform,in.grain_offset.xy);
        let grain_point=floor(grain_raw*256.+vec2(0.5))/256.;
        var grain_factor=1.;
        if in.texture>0. {
            let grain=media_noise(grain_point);
            let pencil=0.03+0.4*grain*grain;
            grain_factor=mix(1.,pencil,min(in.texture,1.));
            if in.texture>1. {
                let dry=select(0.65,0.,media_noise(grain_point/vec2(1.5,5.))<0.48);
                grain_factor=mix(pencil,dry,in.texture-1.);
            }
        }
        let paper_point=media_point(sample_point,in.paper_transform,in.paper_offset.xy);
        pigment+=grain_factor*media_deposition(grain_point,paper_point,distance/in.radius,in.material,in.medium_aux,in.paper);
    }}
    let optical_depth=density*in.weight*profile*pigment/f32(samples*samples);
    if any(in.point<vec2(0.0)) || any(in.point>=u.document.xy) { discard; }
    let selection_alpha=selection_coverage(in.point);
    if selection_alpha<=0. { discard; }
    // Color is constant within this stroke. Store it once; only density adds.
    return vec4(in.color.rgb, optical_depth);
}
