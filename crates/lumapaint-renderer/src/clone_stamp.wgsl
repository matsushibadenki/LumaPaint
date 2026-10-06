struct Input { dst:vec4<f32>, src:vec4<f32>, options:vec4<f32> }
@group(0) @binding(0) var<storage,read> input:array<Input>;
@group(0) @binding(1) var<storage,read_write> output:array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
 let i=id.x; if i>=arrayLength(&input){return;}
 let p=input[i];let sa=p.src.a*p.options.x;let da=p.dst.a;
 let s=p.src.rgb/max(p.src.a,0.000001);let d=p.dst.rgb/max(da,0.000001);
 var b=s;
 switch u32(p.options.y) {
 case 1u:{b=s*d;} case 2u:{b=vec3(1.)-(vec3(1.)-s)*(vec3(1.)-d);}
 case 3u:{b=select(vec3(1.)-2.*(vec3(1.)-s)*(vec3(1.)-d),2.*s*d,d<=vec3(0.5));}
 case 4u:{b=min(s,d);} case 5u:{b=max(s,d);} default:{}
 }
 let rgb=p.dst.rgb*(1.-sa)+sa*((1.-da)*s+da*b);
 let rgba=vec4<u32>(floor(clamp(vec4(rgb,sa+da*(1.-sa)),vec4(0.),vec4(1.))*255.+0.5));
 output[i]=rgba.x | rgba.y<<8u | rgba.z<<16u | rgba.w<<24u;
}
