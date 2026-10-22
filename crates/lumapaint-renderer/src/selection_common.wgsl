struct SelectionRegion { paint:vec4<f32>, bounds: vec4<f32>, info: vec4<f32> }
@group(1) @binding(0) var<storage, read> selection_regions: array<SelectionRegion>;
fn rectangle_distance(point: vec2<f32>, bounds: vec4<f32>) -> f32 {
 let q=abs(point-bounds.xy-bounds.zw*0.5)-bounds.zw*0.5; return max(q.x,q.y);
}
fn selection_segment_distance(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>)->f32 {let v=b-a;let t=clamp(dot(p-a,v)/max(dot(v,v),0.000000000001),0.,1.);return length(p-a-t*v);}
fn selection_edge_cross(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>)->bool {if (a.y>p.y)==(b.y>p.y){return false;}return p.x<(b.x-a.x)*(p.y-a.y)/(b.y-a.y)+a.x;}
fn selection_start()->u32 {if selection_regions[0].info.x==8.{return 1u;}return select(0u,2u,selection_regions[0].info.x==7.);}
fn selection_end()->u32 {return select(arrayLength(&selection_regions),u32(selection_regions[0].info.y),selection_regions[0].info.x==7.);}
fn selection_raster_coverage(point:vec2<f32>)->f32 {
 if selection_regions[0].info.x!=7. {return 0.;}
 let header=selection_regions[0];let second=selection_regions[1];
 let pixel=floor(vec2(header.bounds.z*point.x+header.info.z*point.y+second.bounds.x,header.bounds.w*point.x+header.info.w*point.y+second.bounds.y));
 if any(pixel<vec2(0.))||any(pixel>=header.bounds.xy){return 0.;}
 let i=u32(pixel.y)*u32(header.bounds.x)+u32(pixel.x);let r=selection_regions[u32(second.bounds.z)+i/32u];let word=(i%32u)/4u;
 var bits=0u;if word<4u{bits=bitcast<u32>(r.bounds[word]);}else{bits=bitcast<u32>(r.info[word-4u]);}
 return f32((bits>>((i%4u)*8u))&255u)/255.;
}
fn selection_distance(point:vec2<f32>)->f32 {
 var result=select(-100000.,0.5-selection_raster_coverage(point),selection_start()==2u);var nearest=100000.;var parity=false;
 for(var index=selection_start();index<selection_end();index+=1u){
  let r=selection_regions[index];if r.paint.x>0.5{return 0.5-selection_coverage(point);}let op=i32(r.info.x);if op==4{return -100000.;}
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
  switch op {case 0:{result=distance;}case 1:{result=min(result,distance);}case 2:{result=max(result,-distance);}case 3:{result=max(-result,distance);}case 5:{result=max(result,distance);}default:{}}
 }
 return result;
}
fn selection_coverage(point:vec2<f32>)->f32 {
 var inside=selection_raster_coverage(point);var parity=false;var edge=false;var stroke_hit=false;var nearest=100000.;
 for(var index=selection_start();index<selection_end();index+=1u){
  let r=selection_regions[index];let op=i32(r.info.x);if op==4{return 1.;}
  let local=point-r.bounds.xy;var hit=all(local>=vec2(0.))&&all(local<r.bounds.zw);
  if r.info.y==1.{let p=local/r.bounds.zw*2.-vec2(1.);hit=dot(p,p)<=1.;}
  if r.info.y>=2.{let d=selection_segment_distance(point,r.bounds.xy,r.bounds.zw);nearest=min(nearest,d);
   if r.info.y==2.{parity=parity!=selection_edge_cross(point,r.bounds.xy,r.bounds.zw);edge=edge!=(d<0.0001&&length(point-r.bounds.zw)>=0.0001);}else{stroke_hit=stroke_hit||d<=r.info.z;}
   if r.info.w<0.5{continue;}
   hit=select(stroke_hit,parity||edge,r.info.y==2.);parity=false;edge=false;stroke_hit=false;
  }
  if r.paint.x>0.5 {
    let inner=r.info.z*r.paint.z;let edge=max(r.info.z+1.,inner+1.);let t=clamp((nearest-inner)/(edge-inner),0.,1.);let alpha=1.-t*t*(3.-2.*t);inside=mix(inside,r.paint.y,alpha);nearest=100000.;continue;
  }
  nearest=100000.;let coverage=select(0.,1.,hit);
  switch op {case 0:{inside=coverage;}case 1:{inside=max(inside,coverage);}case 2:{inside*=1.-coverage;}case 3:{inside=coverage*(1.-inside);}case 5:{inside=min(inside,coverage);}default:{}}
 }
 return inside;
}

fn selection_contains(point:vec2<f32>)->bool {return selection_coverage(point)>=0.5;}

fn quick_mask_mode()->bool {if selection_regions[0].info.x==8.{return true;}if selection_regions[0].info.x==7.{return selection_regions[1].info.w>0.5;}return false;}
