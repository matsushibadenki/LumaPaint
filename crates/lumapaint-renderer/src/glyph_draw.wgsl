struct Glyph {rect:vec4<f32>,atlas:vec4<f32>,color:vec4<f32>,clip:vec4<f32>}
@group(0) @binding(0) var masks:texture_2d<f32>;
@group(0) @binding(1) var<uniform> viewport:vec4<f32>;
@group(0) @binding(2) var<storage,read> glyphs:array<Glyph>;
struct Out {@builtin(position) position:vec4<f32>,@location(0) @interpolate(flat) index:u32}
@vertex fn vs_main(@builtin(vertex_index) v:u32,@builtin(instance_index) i:u32)->Out {
 let q=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.))[v];
 let g=glyphs[i];let p=g.rect.xy+q*g.rect.zw;var o:Out;
 o.position=vec4(p.x*2./viewport.x-1.,1.-p.y*2./viewport.y,0.,1.);o.index=i;return o;
}
fn encoded(o:Out)->vec4<f32> {
 let g=glyphs[o.index];let p=o.position.xy;
 if any(p<g.clip.xy)||any(p>=g.clip.zw){discard;}
 let local=vec2<i32>(floor(p-g.rect.xy));
 let alpha=textureLoad(masks,vec2<i32>(g.atlas.xy)+local,0).r*g.color.a;
 return vec4(g.color.rgb*alpha,alpha);
}
@fragment fn fs_encoded(o:Out)->@location(0) vec4<f32>{return encoded(o);}
@fragment fn fs_main(o:Out)->@location(0) vec4<f32>{let p=encoded(o);let c=p.rgb;return vec4(select(c/12.92,pow((c+.055)/1.055,vec3(2.4)),c>vec3(.04045)),p.a);}
