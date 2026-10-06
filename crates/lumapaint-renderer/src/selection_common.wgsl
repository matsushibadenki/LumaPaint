struct SelectionRegion { bounds: vec4<f32>, info: vec4<f32> }
@group(1) @binding(0) var<storage, read> selection_regions: array<SelectionRegion>;
fn rectangle_distance(point: vec2<f32>, bounds: vec4<f32>) -> f32 {
 let q=abs(point-bounds.xy-bounds.zw*0.5)-bounds.zw*0.5; return max(q.x,q.y);
}
fn selection_segment_distance(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>)->f32 {let v=b-a;let t=clamp(dot(p-a,v)/max(dot(v,v),0.000000000001),0.,1.);return length(p-a-t*v);}
fn selection_edge_cross(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>)->bool {if (a.y>p.y)==(b.y>p.y){return false;}return p.x<(b.x-a.x)*(p.y-a.y)/(b.y-a.y)+a.x;}
fn selection_distance(point:vec2<f32>)->f32 {
 var result=-100000.;var nearest=100000.;var parity=false;
 for(var index=0u;index<arrayLength(&selection_regions);index+=1u){
  let r=selection_regions[index];let op=i32(r.info.x);if op==4{return -100000.;}
  var distance=rectangle_distance(point,r.bounds);
  if r.info.y==1. {let radius=r.bounds.zw*0.5;distance=(length((point-r.bounds.xy-radius)/radius)-1.)*min(radius.x,radius.y);}
  if r.info.y>=2. {
    let d=selection_segment_distance(point,r.bounds.xy,r.bounds.zw);
    nearest=min(nearest,d);
    if r.info.y==2. {parity=parity!=selection_edge_cross(point,r.bounds.xy,r.bounds.zw);}
    if r.info.w<0.5 {continue;}
    distance=nearest-r.info.z;
    if r.info.y==2. {distance=select(nearest,-nearest,parity);}
    nearest=100000.;parity=false;
  }
  switch op {case 0:{result=distance;}case 1:{result=min(result,distance);}case 2:{result=max(result,-distance);}case 3:{result=max(-result,distance);}default:{}}
 }
 return result;
}
fn selection_contains(point:vec2<f32>)->bool {
 var inside=false;var parity=false;var edge=false;var stroke_hit=false;
 for(var index=0u;index<arrayLength(&selection_regions);index+=1u){
  let r=selection_regions[index];let op=i32(r.info.x);if op==4{return true;}
  let local=point-r.bounds.xy;var hit=all(local>=vec2(0.))&&all(local<r.bounds.zw);
  if r.info.y==1.{let p=local/r.bounds.zw*2.-vec2(1.);hit=dot(p,p)<=1.;}
  if r.info.y>=2.{let d=selection_segment_distance(point,r.bounds.xy,r.bounds.zw);
   if r.info.y==2.{parity=parity!=selection_edge_cross(point,r.bounds.xy,r.bounds.zw);edge=edge||d<0.0001;}else{stroke_hit=stroke_hit||d<=r.info.z;}
   if r.info.w<0.5{continue;}
   hit=select(stroke_hit,parity||edge,r.info.y==2.);parity=false;edge=false;stroke_hit=false;
  }
  switch op {case 0:{inside=hit;}case 1:{inside=inside||hit;}case 2:{inside=inside&&!hit;}case 3:{inside=hit&&!inside;}default:{}}
 }
 return inside;
}
