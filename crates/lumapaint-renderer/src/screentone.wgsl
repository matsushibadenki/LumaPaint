// Matches core/screentone.rs. Integer noise keeps exports and viewports deterministic.
fn tone_hash(x:u32,y:u32,seed:u32)->f32 {
 var h=x*1597334677u ^ y*3812015801u ^ seed*2798796415u;
 h=(h^(h>>16u))*2246822519u; h=h^(h>>13u);return f32(h&65535u)/65536.;
}
fn tone_pixel(packed:u32,i:u32)->u32 {
 let k=u32(config[1096]); if k==0u || packed>>24u==0u {return packed;}
 let n=u32(config[1097]);let width=max(u32(config[1120]),1u);
 let p=vec2(config[1121],config[1122])+(vec2(f32(i%width),f32(i/width))+vec2(0.5))*vec2(config[1123],config[1124])-vec2(config[1103],config[1104]);
 let x=p.x;let y=p.y;let r=config[1102]*0.017453292519943295;let s=sin(r);let c=cos(r);
 let period=config[1099]/config[1098];let u=(x*c+y*s)/period;let v=(-x*s+y*c)/period;
 let a=abs(fract(u)-0.5);let b=abs(fract(v)-0.5);
 let alpha=packed>>24u;let rgb=vec3<f32>(f32(packed&255u),f32((packed>>8u)&255u),f32((packed>>16u)&255u))/255.;
 var d=config[1100]/100.;
 if k==2u {
  var t=(x*c+y*s)/config[1106];
  if n==1u {t=length(p)/config[1106];} else if n==2u {t=abs(x*c+y*s)/config[1106];}else if n==3u {t=sin((x*c+y*s)/config[1106]*3.141592653589793)*0.5+0.5;}
  d=mix(d,config[1105]/100.,clamp(t,0.,1.));
 }
 if config[1113]>0.5 || k==13u {d*=1.-dot(rgb,vec3(0.2126,0.7152,0.0722));}
 if config[1114]>0.5 {d=1.-d;}
 var dots=3.141592653589793*(a*a+b*b);
 if n==1u {dots=4.*max(a,b)*max(a,b);}else if n==2u {dots=2.*(a+b)*(a+b);}else if n==3u {dots=min(a*a*2.+b*b*6.,1.);}
 let line=2.*b;let noise=tone_hash(bitcast<u32>(i32(floor(u))),bitcast<u32>(i32(floor(v))),u32(config[1107]));
 var score=dots;
 switch k {
 case 2u:{score=3.141592653589793*(a*a+b*b);}
 case 3u:{score=line;if n==1u {score=abs(2.*fract(v+0.2*sin(u*1.5))-1.);}else if n==2u {score=max(line,select(1.,0.,fract(u)<0.65));}else if n==3u {score=abs(line-0.45)*2.;}}
 case 4u:{score=2.*min(a,b);if n==1u {score=2.*abs(a+b-0.35);}else if n==2u {let theta=atan2(fract(v)-0.5,fract(u)-0.5);score=min(sqrt(a*a+b*b)/(0.3+0.12*cos(theta*5.)),1.);}else if n==3u {score=f32(i32(floor(u)+floor(v))&1);}}
 case 5u:{score=(sin(u+v*0.5)+1.)*0.5;if n==1u {score=(sin(u*0.8)*cos(v*0.8)+1.)*0.5;}else if n==2u {score=sin(sqrt(u*u+v*v)*1.4)*0.5+0.5;}else if n==3u {score=(sin(u*0.4)+sin(v*0.6)+2.)*0.25;}}
 case 6u:{let scale=f32(n+1u);score=tone_hash(bitcast<u32>(i32(floor(u*scale))),bitcast<u32>(i32(floor(v*scale))),u32(config[1107]));}
 case 7u:{score=2.*min(a,b);if n==1u {score=2.*min(min(a,b),abs(a-b));}else if n==2u {let jitter=noise*0.3;score=min(2.*b+jitter,2.*a+0.3-jitter);}else if n==3u {score=min(max(2.*b,select(1.,0.,(i32(floor(u))&1)==0)),max(2.*a,select(1.,0.,(i32(floor(v))&1)==1)));}}
 case 8u:{score=abs(sin(atan2(v,u)*24.));if n==1u {score=abs(sin(u*0.8));}else if n==2u {score=abs(sin(sqrt(u*u+v*v)));}else if n==3u {score=min(abs(sin(atan2(v,u)*12.))+sqrt(u*u+v*v)*0.01,1.);}}
 case 9u:{score=max(2.*b,abs(sin(u*0.3)));if n==1u {score=min(2.*b,max(abs(2.*fract(u+select(0.5,0.,(i32(floor(v))&1)==0))-1.),0.2));}else if n==2u {score=(sin(u*0.2)+sin(v*0.3+sin(u*0.1))+2.)*0.25;}else if n==3u {score=abs(sin(v*2.+sin(u*0.3)));}}
 case 11u:{score=min(dots*0.7+noise*0.6,1.);if n==1u {score=min(dots+noise*0.4,1.);}else if n==2u {score=min(line+noise*0.5,1.);}else if n==3u {score=min(2.*min(a,b)+noise*0.35,1.);}}
 default:{}
 }
 let ink=d>=1. || (d>0. && score<d*config[1101]*config[1101]);
 if !ink && config[1112]<0.5 {return 0u;}
 var color=vec3<u32>(u32(config[1108]),u32(config[1109]),u32(config[1110]));
 if !ink || k==10u {color=vec3(255u);}
 let out=(color*alpha+127u)/255u;return out.x|out.y<<8u|out.z<<16u|alpha<<24u;
}
