fn mask_distance(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>)->f32 {
 let delta=b-a;let d=dot(delta,delta);if d==0. {return distance(p,a);}
 return distance(p,a+clamp(dot(p-a,delta)/d,0.,1.)*delta);
}
struct MaskSelectionResult { inside:bool, next:u32 }
fn mask_selection(offset:u32,count:u32,p:vec2<f32>)->MaskSelectionResult {
  var base=offset;var inside=false;
  for(var region=0u;region<count;region++){
   let shape=u32(config[base]);let op=u32(config[base+1u]);let radius=config[base+2u];let n=u32(config[base+3u]);
   let origin=vec2(config[base+4u],config[base+5u]);let size=vec2(config[base+6u],config[base+7u]);let pts=base+8u;var hit=false;
   if shape==0u{hit=all(p>=origin)&&all(p<origin+size);}
   else if shape==1u{let q=(p-origin-size*0.5)/(size*0.5);hit=dot(q,q)<=1.;}
   else{
    for(var j=0u;j<n;j++){
     let a=vec2(config[pts+j*2u],config[pts+j*2u+1u]);let k=(j+1u)%n;let b=vec2(config[pts+k*2u],config[pts+k*2u+1u]);
     if shape==2u{
      if mask_distance(p,a,b)<0.0001{hit=true;break;}
      if (a.y>p.y)!=(b.y>p.y) && p.x<(b.x-a.x)*(p.y-a.y)/(b.y-a.y)+a.x{hit=!hit;}
     }else{
      if (j+1u<n && mask_distance(p,a,b)<=radius) || (j==0u && distance(p,a)<=radius){hit=true;break;}
     }
    }
   }
   switch op{case 0u:{inside=hit;}case 1u:{inside=inside||hit;}case 2u:{inside=inside&&!hit;}case 3u:{inside=inside&&hit;}default:{inside=hit&&!inside;}}
   base=pts+n*2u;
  }
 return MaskSelectionResult(inside,base);
}
fn mask_pixel_coordinate(p:vec2<f32>)->vec2<f32> {
 let nearest=round(p);
 return select(floor(p),nearest,abs(p-nearest)<=max(abs(p)*0.0000002,vec2(0.00001)));
}
fn mask_coverage(i:u32)->f32 {
 let kind=u32(config[1125]);if kind==0u{return 1.;}
 let width=max(u32(config[1120]),1u);
 let world=vec2(config[1121],config[1122])+(vec2(f32(i%width),f32(i/width))+vec2(0.5))*vec2(config[1123],config[1124]);
 let p=vec2(config[1135]*world.x+config[1137]*world.y+config[1139],config[1136]*world.x+config[1138]*world.y+config[1140]);
 var tone=-1.;var inside=false;let count=u32(config[1130]);
 if kind==1u {
  let q=mask_pixel_coordinate(p);let w=u32(config[1128]);
  if q.x>=0. && q.y>=0. && q.x<config[1128] && q.y<config[1129] {
   let index=u32(q.y)*w+u32(q.x);var lo=0u;var hi=count;
   loop{if lo>=hi{break;}let mid=(lo+hi)/2u;if bitcast<u32>(config[1141u+mid*2u])<=index{lo=mid+1u;}else{hi=mid;}}
   if lo>0u{let base=1141u+(lo-1u)*2u;inside=index<bitcast<u32>(config[base])+bitcast<u32>(config[base+1u]);}
   let start=1141u+count*2u;lo=0u;hi=u32(config[1131]);
   loop{if lo>=hi{break;}let mid=(lo+hi)/2u;if bitcast<u32>(config[start+mid*3u])<=index{lo=mid+1u;}else{hi=mid;}}
   if lo>0u{let base=start+(lo-1u)*3u;if index<bitcast<u32>(config[base])+bitcast<u32>(config[base+1u]){tone=f32(bitcast<u32>(config[base+2u]))/255.;}}

  }
 }else{
  var result=mask_selection(1141u,count,p);inside=result.inside;var base=result.next;
  for(var edit=0u;edit<u32(config[1131]);edit++){
   let reveal=config[base]>0.5;
   let q=vec2(config[base+1u]*p.x+config[base+3u]*p.y+config[base+5u],config[base+2u]*p.x+config[base+4u]*p.y+config[base+6u]);
   let n=u32(config[base+7u]);let clips=u32(config[base+8u]);
   let region=mask_selection(base+9u,n,q);let clip=mask_selection(region.next,clips,q);
   if region.inside && (clips==0u || clip.inside) {inside=reveal;}
   base=clip.next;
  }
 }
 var value=select(0.,1.,inside);if tone>=0.{value=tone;}if config[1126]>0.5{value=1.-value;}
 return 1.-config[1127]+config[1127]*value;
}
fn adjust(packed:u32,i:u32)->u32 {
 let color=adjust_color(packed,i);let alpha=color>>24u;
 let coverage=mask_coverage(i);if coverage==1.{return color;}
 let result_alpha=u32(floor(f32(alpha)*coverage+0.5));
 if alpha==0u{return 0u;}
 let rgb=vec3(color&255u,(color>>8u)&255u,(color>>16u)&255u);
 let result=(rgb*result_alpha+vec3(alpha/2u))/alpha;
 return result.x|result.y<<8u|result.z<<16u|result_alpha<<24u;
}
