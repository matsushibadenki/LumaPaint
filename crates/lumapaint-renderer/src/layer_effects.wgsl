// Packed premultiplied RGBA8; tone/curve tables prepared by the portable core.
@group(0) @binding(0) var<storage,read> input:array<u32>;
@group(0) @binding(1) var<storage,read_write> output:array<u32>;
@group(0) @binding(2) var<storage,read> config:array<f32>;
fn rem(x:f32,d:f32)->f32 {let r=x%d;return select(r,r+d,r<0.);}
fn from_hsl(h:f32,s:f32,l:f32)->vec3<f32>{
 let c=(1.-abs(2.*l-1.))*s;let x=c*(1.-abs(rem(h/60.,2.)-1.));var rgb=vec3(c,0.,x);
 switch u32(h/60.) {case 0u:{rgb=vec3(c,x,0.);}case 1u:{rgb=vec3(x,c,0.);}case 2u:{rgb=vec3(0.,c,x);}case 3u:{rgb=vec3(0.,x,c);}case 4u:{rgb=vec3(x,0.,c);}default:{}}
 return rgb+vec3(l-c/2.);
}
fn to_hsl(rgb:vec3<f32>)->vec3<f32>{
 let hi=max(rgb.x,max(rgb.y,rgb.z));let lo=min(rgb.x,min(rgb.y,rgb.z));let d=hi-lo;let l=(hi+lo)/2.;if d==0. {return vec3(0.,0.,l);}
 var h=(rgb.x-rgb.y)/d+4.;if hi==rgb.x {h=rem((rgb.y-rgb.z)/d,6.);}else if hi==rgb.y {h=(rgb.z-rgb.x)/d+2.;}
 return vec3(h*60.,d/(1.-abs(2.*l-1.)),l);
}
fn curve(channel:u32,x:f32)->f32{
 let base=776u+channel*68u;let n=u32(config[base]);let points=base+4u;
 if x<=config[points] {return config[points+1u];}let end=points+(n-1u)*4u;if x>=config[end] {return config[end+1u];}
 var p=points;for(var j=1u;j<n;j++){if x<=config[points+j*4u] {p=points+(j-1u)*4u;break;}}
 let x0=config[p];let y0=config[p+1u];let x1=config[p+4u];let y1=config[p+5u];let h=x1-x0;let t=(x-x0)/h;
 if config[base+1u]<0.5 || n==2u {return y0+t*(y1-y0);}
 let m0=config[p+2u];let m1=config[p+6u];let t2=t*t;let t3=t2*t;
 return clamp((2.*t3-3.*t2+1.)*y0+(t3-2.*t2+t)*h*m0+(-2.*t3+3.*t2)*y1+(t3-t2)*h*m1,0.,1.);
}
fn adjust_color(packed:u32,i:u32)->u32 {
 let a=packed>>24u;if a==0u {return packed;}
 let rgba=vec3(packed&255u,(packed>>8u)&255u,(packed>>16u)&255u);let straight=min((rgba*255u+vec3(a/2u))/a,vec3(255u));
 var rgb=vec3(config[8u+straight.x],config[264u+straight.y],config[520u+straight.z]);
 let gray=rgb.x*0.2126+rgb.y*0.7152+rgb.z*0.0722;let chroma=max(rgb.x,max(rgb.y,rgb.z))-min(rgb.x,min(rgb.y,rgb.z));let saturation=max(1.+config[0]+config[1]*(1.-clamp(chroma,0.,1.)),0.);
 for(var c=0u;c<3u;c++){rgb[c]=curve(c+1u,curve(0u,clamp(gray+(rgb[c]-gray)*saturation,0.,1.)));}
 if config[4]>0.5 {
  let hsl=to_hsl(rgb);if hsl.y>0. {
   let centers=array<f32,9>(0.,30.,60.,120.,180.,240.,270.,300.,360.);var index=7u;for(var j=0u;j<8u;j++){if hsl.x<=centers[j+1u]{index=j;break;}}
   let t=(hsl.x-centers[index])/(centers[index+1u]-centers[index]);var offset=vec3(0.);for(var c=0u;c<3u;c++){offset[c]=config[1048u+index*3u+c]*(1.-t)+config[1048u+((index+1u)%8u)*3u+c]*t;}
   rgb=from_hsl(rem(hsl.x+offset.x*0.3,360.),clamp(hsl.y*(1.+offset.y/100.),0.,1.),clamp(hsl.z+offset.z/100.*hsl.y,0.,1.));
  }
 }
 if config[5]>0.5 {
  var l=clamp(rgb.x*0.2126+rgb.y*0.7152+rgb.z*0.0722,0.,1.);l=clamp(l+l*(1.-l)*config[3]/50.,0.,1.);
  var weights=vec3((1.-l)*(1.-l),2.*l*(1.-l),l*l);
  if config[6]<0.5 {weights=pow(weights,vec3(config[2]));weights/=weights.x+weights.y+weights.z;}
  let original=rgb;for(var z=0u;z<3u;z++){let b=1072u+z*4u;let tint=vec3(config[1084u+z*4u],config[1085u+z*4u],config[1086u+z*4u]);rgb+=weights[z]*(config[b+1u]/100.*(tint-original)+vec3(config[b+2u]/100.));}
 }
 let adjusted=vec3<u32>(floor(clamp(rgb,vec3(0.),vec3(1.))*255.+0.5));if config[1096]>0.5 {return tone_pixel(adjusted.x|adjusted.y<<8u|adjusted.z<<16u|a<<24u,i);}
 let result=(adjusted*a+127u)/255u;return result.x|result.y<<8u|result.z<<16u|a<<24u;
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let i=id.x;if i>=arrayLength(&input){return;}output[i]=adjust(input[i],i);
}
// Display-only output has aligned rows for copy_buffer_to_texture. Quantize
// opacity exactly where the CPU workspace path does, before sRGB texture sampling.
@compute @workgroup_size(256)
fn display(@builtin(global_invocation_id) id:vec3<u32>){
 let i=id.x;if i>=arrayLength(&input){return;}
 let packed=adjust(input[i],i);let opacity=config[1134u];
 let rgba=vec4<f32>(f32(packed&255u),f32((packed>>8u)&255u),f32((packed>>16u)&255u),f32(packed>>24u));
 let result=vec4<u32>(floor(rgba*opacity+0.5));
 let width=u32(config[1132u]);let stride=u32(config[1133u]);
 output[(i/width)*stride+i%width]=result.x|result.y<<8u|result.z<<16u|result.w<<24u;
}
