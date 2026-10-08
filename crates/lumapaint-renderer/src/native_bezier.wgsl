struct Segment { a: vec4<f32>, b: vec4<f32>, parameters:vec4<f32> }
struct Shape { origin: vec4<f32>, inverse: vec4<f32>, rectangle: vec4<f32>, color: vec4<f32>, parameters: vec4<f32> }
@group(0) @binding(0) var<uniform> shape: Shape;
@group(0) @binding(1) var<storage, read> curves: array<Segment>;
struct Out { @builtin(position) position: vec4<f32> }
@vertex fn vs_main(@builtin(vertex_index) index:u32)->Out {
 let corners=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
 let screen=shape.rectangle.xy+corners[index]*shape.rectangle.zw;
 var out:Out;out.position=vec4(screen.x/shape.origin.z*2.-1.,1.-screen.y/shape.origin.w*2.,0.,1.);return out;
}
fn point(s:Segment,t:f32)->vec2<f32> {
 let v=1.-t;
 if s.parameters.y>.5 {let w=s.parameters.x;return (v*v*s.a.xy+2.*w*v*t*s.a.zw+t*t*s.b.xy)/(v*v+2.*w*v*t+t*t);}
 return v*v*v*s.a.xy+3.*v*v*t*s.a.zw+3.*v*t*t*s.b.xy+t*t*t*s.b.zw;
}
fn derivative(s:Segment,t:f32)->vec2<f32> {
 let v=1.-t;
 if s.parameters.y>.5 {
  let w=s.parameters.x;let n=v*v*s.a.xy+2.*w*v*t*s.a.zw+t*t*s.b.xy;
  let d=v*v+2.*w*v*t+t*t;let nd=2.*(v*(w*s.a.zw-s.a.xy)+t*(s.b.xy-w*s.a.zw));let dd=2.*(w-1.)*(1.-2.*t);
  return (nd*d-n*dd)/(d*d);
 }
 return 3.*(v*v*(s.a.zw-s.a.xy)+2.*v*t*(s.b.xy-s.a.zw)+t*t*(s.b.zw-s.b.xy));
}
fn second(s:Segment,t:f32)->vec2<f32>{
 if s.parameters.y>.5 {
  let v=1.-t;let w=s.parameters.x;let n=v*v*s.a.xy+2.*w*v*t*s.a.zw+t*t*s.b.xy;let d=v*v+2.*w*v*t+t*t;
  let nd=2.*(v*(w*s.a.zw-s.a.xy)+t*(s.b.xy-w*s.a.zw));let dd=2.*(w-1.)*(1.-2.*t);
  let ndd=2.*(s.a.xy-2.*w*s.a.zw+s.b.xy);let ddd=4.*(1.-w);
  return (ndd*d-n*ddd)/(d*d)-2.*dd*(nd*d-n*dd)/(d*d*d);
 }
 return 6.*((1.-t)*(s.b.xy-2.*s.a.zw+s.a.xy)+t*(s.b.zw-2.*s.b.xy+s.a.zw));
}
fn encoded(c:vec3<f32>)->vec3<f32> {
 return select(c*12.92,1.055*pow(max(c,vec3(0.)),vec3(1./2.4))-.055,c>vec3(.0031308));
}
fn linear(c:vec3<f32>)->vec3<f32> {
 return select(c/12.92,pow((c+.055)/1.055,vec3(2.4)),c>vec3(.04045));
}
// Integrate a locally straight boundary over a square screen pixel instead of
// treating every edge orientation as a one-dimensional coverage ramp.
fn pixel_coverage(distance:f32, gradient:vec2<f32>)->f32 {
 let dx=abs(gradient.x);let dy=abs(gradient.y);
 let a=max(max(dx,dy),1e-12);let b=min(dx,dy);
 let x=clamp(-distance+(a+b)*.5,0.,a+b);
 if b<1e-8 {return clamp(x/a,0.,1.);}
 if x<b {return x*x/(2.*a*b);}
 if x<a {return (x-b*.5)/a;}
 let tail=a+b-x;return 1.-tail*tail/(2.*a*b);
}
struct Paint { inverse:vec4<f32>, origin:vec4<f32>, parameters:vec4<f32>, reserved:vec4<f32> }
@group(0) @binding(2) var<uniform> paint:Paint;
@group(0) @binding(3) var<storage,read> stops:array<vec4<f32>>;
@group(0) @binding(4) var<storage,read> clips:array<Shape>;
@group(0) @binding(5) var<storage,read> clip_curves:array<Segment>;
@group(0) @binding(6) var masks:texture_2d_array<f32>;
fn segment_at(i:u32,clip:bool)->Segment {if clip {return clip_curves[i];}return curves[i];}
fn path_coverage(p:vec2<f32>,inverse:vec4<f32>,count:u32,start:u32,even_odd:bool,exact:bool,analytic:bool,clip:bool)->f32 {
 if exact {
  // The qualified fill/stroke contours are axis-aligned rectangles.
  // Integrate the physical pixel footprint exactly, including its corners.
  let pixel=abs(vec2(inverse.x,inverse.w));
  var coverage=0.;
  for(var i=start;i<start+count;i+=4u) {
   var lo=vec2(1e30);var hi=vec2(-1e30);
   for(var j=0u;j<4u;j++) {lo=min(lo,segment_at(i+j,clip).a.xy);hi=max(hi,segment_at(i+j,clip).a.xy);}
   let extent=max(vec2(0.),min(hi,p+pixel*.5)-max(lo,p-pixel*.5));
   coverage+=extent.x*extent.y/(pixel.x*pixel.y);
  }
  return clamp(coverage,0.,1.);
 }
 var winding=0;var distance2=1e30;var closest_delta=vec2(0.);
 for(var i=start;i<start+count;i++) {
  let s=segment_at(i,clip);
  let edge=s.b.zw-s.a.xy;
  let c1=s.a.xy+edge/3.;let c2=s.a.xy+edge*2./3.;
  if s.parameters.y<.5 && all(abs(s.a.zw-c1)<vec2(1e-4)) && all(abs(s.b.xy-c2)<vec2(1e-4)) {
   let t=clamp(dot(p-s.a.xy,edge)/max(dot(edge,edge),1e-20),0.,1.);
   let q=s.a.xy+t*edge-p;if dot(q,q)<distance2 {distance2=dot(q,q);closest_delta=q;}
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
   let q=point(s,t)-p;if dot(q,q)<distance2 {distance2=dot(q,q);closest_delta=q;}
  }
  // Split at the y derivative's roots. Each interval is monotonic, so
  // bisection finds every ray crossing without tessellation artifacts.
  var a=-s.a.y+3.*s.a.w-3.*s.b.y+s.b.w;var b=2.*(s.a.y-2.*s.a.w+s.b.y);var c=s.a.w-s.a.y;
  if s.parameters.y>.5 {
   let w=s.parameters.x;let na=s.a.y-2.*w*s.a.w+s.b.y;let nb=2.*(w*s.a.w-s.a.y);let nc=s.a.y;
   let da=2.*(1.-w);let db=2.*(w-1.);
   a=na*db-nb*da;b=2.*(na-nc*da);c=nb-nc*db;
  }
  var cuts=array<f32,4>(0.,1.,1.,1.);var n=2u;
  if abs(a)>1e-12 {let disc=b*b-4.*a*c;if disc>0. {let root=sqrt(disc);let r0=(-b-root)/(2.*a);let r1=(-b+root)/(2.*a);if r0>0. && r0<1. {cuts[n]=r0;n++;}if r1>0. && r1<1. {cuts[n]=r1;n++;}}}
  else if abs(b)>1e-12 {let r=-c/b;if r>0. && r<1. {cuts[n]=r;n++;}}
  for(var j=1u;j<n;j++){var k=j;loop{if k==0u || cuts[k-1u]<=cuts[k]{break;}let v=cuts[k];cuts[k]=cuts[k-1u];cuts[k-1u]=v;k--;}}
  for(var j=0u;j+1u<n;j++) {
   var lo=cuts[j];var hi=cuts[j+1u];let y0=point(s,lo).y;let y1=point(s,hi).y;
   if p.y>=min(y0,y1) && p.y<max(y0,y1) {let up=y1>y0;for(var k=0u;k<24u;k++){let mid=(lo+hi)*.5;if (point(s,mid).y<p.y)==up {lo=mid;}else{hi=mid;}}if point(s,(lo+hi)*.5).x>p.x {winding+=select(-1,1,up);}}
  }
 }
 let inside=select(winding!=0,(abs(winding)%2)==1,even_odd);
 let signed_distance=select(sqrt(distance2),-sqrt(distance2),inside);
 let normal=closest_delta/max(sqrt(distance2),1e-12);
 let gradient=vec2(dot(normal,vec2(inverse.x,inverse.z)),dot(normal,vec2(inverse.y,inverse.w)));
 // Analytic normals are qualified for positive uniform-scale fills only.
 // Keep finite differences for strokes and other transforms.
 let stroke_gradient=vec2(dpdx(signed_distance),dpdy(signed_distance));
 let coverage=pixel_coverage(signed_distance,select(gradient,stroke_gradient,!analytic));
 return coverage;
}

fn painted(p:vec2<f32>,coverage:f32)->vec4<f32> {
 if paint.origin.z<.5 {let alpha=shape.color.a*coverage;return vec4(encoded(shape.color.rgb)*alpha,alpha);}
 let delta=p-paint.origin.xy;
 let q=vec2(dot(paint.inverse.xy,delta),dot(paint.inverse.zw,delta));
 let t=select(q.x,length(q),paint.origin.z>1.5);
 let count=u32(paint.origin.w);
 var lo=0u;var hi=count;
 loop {if lo>=hi {break;}let mid=(lo+hi)/2u;if stops[mid*2u+1u].x<=t {lo=mid+1u;}else{hi=mid;}}
 var color=stops[0];
 if lo>=count {color=stops[(count-1u)*2u];}
 else if lo>0u {
  let left=stops[(lo-1u)*2u];let right=stops[lo*2u];
  let start=stops[(lo-1u)*2u+1u].x;let end=stops[lo*2u+1u].x;
  let ratio=clamp((t-start)/max(end-start,1e-12),0.,1.);
  color=mix(left,right,ratio);
 }
 let alpha=color.a*coverage*paint.parameters.x;
 return vec4(color.rgb*alpha,alpha);
}
fn draw_native(position:vec4<f32>)->vec4<f32> {
 let delta=position.xy-shape.origin.xy;
 let p=vec2(dot(shape.inverse.xy,delta),dot(shape.inverse.zw,delta));
 var coverage=path_coverage(p,shape.inverse,u32(shape.parameters.x),0u,shape.parameters.y>.5,shape.parameters.z>.5,shape.parameters.w<.5,false);
 for(var i=0u;i<u32(paint.parameters.y);i++) {
  let c=clips[i];
  if c.parameters.z>1.5 {
   let p=position.xy-c.rectangle.xy;
   var alpha=0.;if all(p>=vec2(0.)) && all(p<c.rectangle.zw) {alpha=textureLoad(masks,vec2<i32>(p),i32(i),0).r;}
   coverage*=alpha;continue;
  }
  let delta=position.xy-c.origin.xy;
  let cp=vec2(dot(c.inverse.xy,delta),dot(c.inverse.zw,delta));
  coverage*=path_coverage(cp,c.inverse,u32(c.parameters.x),u32(c.parameters.w),c.parameters.y>.5,c.parameters.z>.5,c.inverse.y==0. && c.inverse.z==0. && c.inverse.x==c.inverse.w && c.inverse.x>0.,true);
 }
 return painted(p,coverage);
}

@fragment fn fs_main(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{let c=draw_native(p);return vec4(linear(c.rgb),c.a);}
@fragment fn fs_encoded(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{return draw_native(p);}
