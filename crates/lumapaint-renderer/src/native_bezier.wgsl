struct Segment { a: vec4<f32>, b: vec4<f32> }
struct Shape { origin: vec4<f32>, inverse: vec4<f32>, rectangle: vec4<f32>, color: vec4<f32>, parameters: vec4<f32> }
@group(0) @binding(0) var<uniform> shape: Shape;
@group(0) @binding(1) var<storage, read> curves: array<Segment>;
struct Out { @builtin(position) position: vec4<f32> }
@vertex fn vs_main(@builtin(vertex_index) index:u32)->Out {
 let corners=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
 let screen=shape.rectangle.xy+corners[index]*shape.rectangle.zw;
 var out:Out;out.position=vec4(screen.x/shape.origin.z*2.-1.,1.-screen.y/shape.origin.w*2.,0.,1.);return out;
}
fn point(s:Segment,t:f32)->vec2<f32> { let v=1.-t;return v*v*v*s.a.xy+3.*v*v*t*s.a.zw+3.*v*t*t*s.b.xy+t*t*t*s.b.zw; }
fn derivative(s:Segment,t:f32)->vec2<f32> {let v=1.-t;return 3.*(v*v*(s.a.zw-s.a.xy)+2.*v*t*(s.b.xy-s.a.zw)+t*t*(s.b.zw-s.b.xy));}
fn second(s:Segment,t:f32)->vec2<f32>{return 6.*((1.-t)*(s.b.xy-2.*s.a.zw+s.a.xy)+t*(s.b.zw-2.*s.b.xy+s.a.zw));}
@fragment fn fs_main(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
 let delta=position.xy-shape.origin.xy;
 let p=vec2(dot(shape.inverse.xy,delta),dot(shape.inverse.zw,delta));
 var winding=0;var distance2=1e30;
 for(var i=0u;i<u32(shape.parameters.x);i++) {
  let s=curves[i];
  let edge=s.b.zw-s.a.xy;
  let c1=s.a.xy+edge/3.;let c2=s.a.xy+edge*2./3.;
  if all(abs(s.a.zw-c1)<vec2(1e-4)) && all(abs(s.b.xy-c2)<vec2(1e-4)) {
   let t=clamp(dot(p-s.a.xy,edge)/max(dot(edge,edge),1e-20),0.,1.);
   let q=s.a.xy+t*edge-p;distance2=min(distance2,dot(q,q));
   if p.y>=min(s.a.y,s.b.w) && p.y<max(s.a.y,s.b.w) {
    let cross_x=s.a.x+(p.y-s.a.y)*edge.x/edge.y;
    if cross_x>p.x {winding+=select(-1,1,edge.y>0.);}
   }
   continue;
  }
  // Closest point on the native cubic, refined with its analytic derivatives.
  for(var sample=0u;sample<5u;sample++) {
   var t=f32(sample)*.25;
   for(var iteration=0u;iteration<7u;iteration++) {let q=point(s,t)-p;let d=derivative(s,t);let denominator=dot(d,d)+dot(q,second(s,t));if abs(denominator)>1e-12 {t=clamp(t-dot(q,d)/denominator,0.,1.);}}
   let q=point(s,t)-p;distance2=min(distance2,dot(q,q));
  }
  // Split at the y derivative's roots. Each interval is monotonic, so
  // bisection finds every ray crossing without tessellation artifacts.
  let a=-s.a.y+3.*s.a.w-3.*s.b.y+s.b.w;let b=2.*(s.a.y-2.*s.a.w+s.b.y);let c=s.a.w-s.a.y;
  var cuts=array<f32,4>(0.,1.,1.,1.);var n=2u;
  if abs(a)>1e-12 {let disc=b*b-4.*a*c;if disc>0. {let root=sqrt(disc);let r0=(-b-root)/(2.*a);let r1=(-b+root)/(2.*a);if r0>0. && r0<1. {cuts[n]=r0;n++;}if r1>0. && r1<1. {cuts[n]=r1;n++;}}}
  else if abs(b)>1e-12 {let r=-c/b;if r>0. && r<1. {cuts[n]=r;n++;}}
  for(var j=1u;j<n;j++){var k=j;loop{if k==0u || cuts[k-1u]<=cuts[k]{break;}let v=cuts[k];cuts[k]=cuts[k-1u];cuts[k-1u]=v;k--;}}
  for(var j=0u;j+1u<n;j++) {
   var lo=cuts[j];var hi=cuts[j+1u];let y0=point(s,lo).y;let y1=point(s,hi).y;
   if p.y>=min(y0,y1) && p.y<max(y0,y1) {let up=y1>y0;for(var k=0u;k<24u;k++){let mid=(lo+hi)*.5;if (point(s,mid).y<p.y)==up {lo=mid;}else{hi=mid;}}if point(s,(lo+hi)*.5).x>p.x {winding+=select(-1,1,up);}}
  }
 }
 let inside=select(winding!=0,(abs(winding)%2)==1,shape.parameters.y>.5);
 let signed_distance=select(sqrt(distance2),-sqrt(distance2),inside);
 let pixel_width=max(fwidth(signed_distance),1e-12);
 let coverage=clamp(.5-signed_distance/max(pixel_width,1e-12),0.,1.);
 let alpha=shape.color.a*coverage;
 return vec4(shape.color.rgb*alpha,alpha);
}
