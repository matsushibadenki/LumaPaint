struct Input { dst:vec4<f32>, options:vec4<f32> }
struct Params { image:vec4<f32>, movement:vec4<f32>, color:vec4<f32> }
@group(0) @binding(0) var<storage,read> input:array<Input>;
@group(0) @binding(1) var<storage,read_write> output:array<u32>;
@group(0) @binding(2) var<storage,read> source:array<vec4<f32>>;
@group(0) @binding(3) var<uniform> params:Params;
fn pixel(x:i32,y:i32)->vec4<f32>{let w=i32(params.image.x);let h=i32(params.image.y);return source[u32(clamp(y,0,h-1)*w+clamp(x,0,w-1))];}
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let i=id.x;if i>=arrayLength(&input){return;}let p=input[i];let x=i32(p.options.x);let y=i32(p.options.y);let center=pixel(x,y);var desired=vec4(0.);
 if params.image.z<1.5 {
   for(var oy=-1;oy<=1;oy++){for(var ox=-1;ox<=1;ox++){let wx=select(1.,2.,ox==0);let wy=select(1.,2.,oy==0);desired+=pixel(x+ox,y+oy)*wx*wy/16.;}}
   if params.image.z>0.5 {var delta=(center.rgb-desired.rgb)*1.5;if params.image.w>0.5{delta=clamp(delta,vec3(-0.15*center.a),vec3(0.15*center.a));}desired=vec4(clamp(center.rgb+delta,vec3(0.),vec3(center.a)),center.a);}
 }else if params.movement.z>0.5 {desired=vec4(params.color.rgb*params.color.a,params.color.a);}
 else{let q=p.options.xy+params.movement.xy;let base=vec2<i32>(floor(q));let f=fract(q);desired=pixel(base.x,base.y)*(1.-f.x)*(1.-f.y)+pixel(base.x+1,base.y)*f.x*(1.-f.y)+pixel(base.x,base.y+1)*(1.-f.x)*f.y+pixel(base.x+1,base.y+1)*f.x*f.y;}
 let rgba=vec4<u32>(floor(clamp(p.dst+(desired-p.dst)*p.options.z,vec4(0.),vec4(1.))*255.+0.5));output[i]=rgba.x|rgba.y<<8u|rgba.z<<16u|rgba.w<<24u;
}
