export type KeyCurve={x1:number;y1:number;x2:number;y2:number};
export const DEFAULT_KEY_CURVE:KeyCurve={x1:1/3,y1:0,x2:2/3,y2:1};
export function sampleKeyCurve(curve:KeyCurve,time:number):number{
 if(time<=0)return 0;if(time>=1)return 1;
 const bezier=(u:number,a:number,b:number)=>3*(1-u)**2*u*a+3*(1-u)*u*u*b+u*u*u;
 let low=0,high=1;for(let i=0;i<24;i++){const u=(low+high)/2;if(bezier(u,curve.x1,curve.x2)<time)low=u;else high=u;}
 return bezier((low+high)/2,curve.y1,curve.y2);
}
export function curveControl(curve:KeyCurve,control:1|2,x:number,y:number,limits={min:0,max:1}):KeyCurve{
 const clamp=(n:number)=>Math.max(0,Math.min(1,n));
 return control===1?{...curve,x1:Math.min(curve.x2,clamp(x)),y1:Math.max(limits.min,Math.min(limits.max,y))}:{...curve,x2:Math.max(curve.x1,clamp(x)),y2:Math.max(limits.min,Math.min(limits.max,y))};
}
export function equalKeyCurve(a?:KeyCurve|null,b?:KeyCurve|null):boolean{
 return !a||!b?!a&&!b:(['x1','y1','x2','y2'] as const).every(k=>Math.fround(a[k])===Math.fround(b[k]));
}

// Same document-unit construction as Rust's atomic LinkedCurve edit.
export function linkedCurves(keys:readonly {frame:number;value:number;interpolation:string;curve?:KeyCurve|null}[],i:number,input:KeyCurve,control:1|2):{index:number;curve:KeyCurve}[]{
 const f=Math.fround,round=(c:KeyCurve):KeyCurve=>({x1:f(c.x1),y1:f(c.y1),x2:f(c.x2),y2:f(c.y2)});
 const curve=round(input);if(i<0||i+1>=keys.length||curveOvershoots(curve))return [];
 const j=control===1?i-1:i+1,result=[];
 if(j>=0&&j+1<keys.length&&keys[j].interpolation==='cubic'){
  const other=round(keys[j].curve??DEFAULT_KEY_CURVE);if(curveOvershoots(other))return [];const dt=keys[i+1].frame-keys[i].frame,dv=keys[i+1].value-keys[i].value,nt=keys[j+1].frame-keys[j].frame,nv=keys[j+1].value-keys[j].value;
  const x=Math.max(f(.0001),control===1?curve.x1:f(1-curve.x2));let y=control===1?curve.y1:f(1-curve.y2);if(dv*nv<=0)y=0;
  if(control===1){curve.x1=x;curve.x2=Math.max(curve.x2,x);curve.y1=y;}else{curve.x2=f(1-x);curve.x1=Math.min(curve.x1,curve.x2);curve.y2=f(1-y);}
  const sx=control===1?curve.x1:f(1-curve.x2),sy=control===1?curve.y1:f(1-curve.y2),slope=dv*sy/(dt*sx),ratio=nv===0?0:Math.abs(slope*nt/nv),nx=f(Math.min(Math.max(f(.0001),control===1?f(1-other.x2):other.x1),ratio>0?1/ratio:1)),ny=f(Math.min(1,ratio*nx));
  if(control===1){other.x2=f(1-nx);other.x1=Math.min(other.x1,other.x2);other.y2=f(1-ny);}else{other.x1=nx;other.x2=Math.max(other.x2,nx);other.y1=ny;}
  result.push({index:j,curve:other});
 }
 result.push({index:i,curve});return result;
}

export const curveOvershoots=(c:KeyCurve)=>c.y1<0||c.y1>1||c.y2<0||c.y2>1;
export function curveValueBounds(a:number,b:number,min:number,max:number,enabled:boolean){
 if(!enabled||a===b)return {min:0,max:1};
 const dv=b-a,low=Math.max(-2,Math.min((min-a)/dv,(max-a)/dv)),high=Math.min(3,Math.max((min-a)/dv,(max-a)/dv));
 // Stay inside physical limits after the handles are quantized to float32.
 return {min:low>-2?Math.min(0,low+1e-6*Math.max(1,Math.abs(low))):low,max:high<3?Math.max(1,high-1e-6*Math.max(1,Math.abs(high))):high};
}

// Same f32 time-handle construction and signed common-slope limits as Rust.
export function overshootLinkedCurves(keys:readonly {frame:number;value:number;interpolation:string;curve?:KeyCurve|null}[],i:number,input:KeyCurve,control:1|2,limits={min:-65536,max:65536},overshoot=true,preserveTime=false):{index:number;curve:KeyCurve}[]{
 const f=Math.fround,round=(c:KeyCurve):KeyCurve=>({x1:f(c.x1),y1:f(c.y1),x2:f(c.x2),y2:f(c.y2)}),curve=round(input),result=[];
 const valid=(c:KeyCurve,a:number,b:number)=>Object.values(c).every(Number.isFinite)&&c.x1>=0&&c.x1<=c.x2&&c.x2<=1&&[c.y1,c.y2].every(y=>{const v=y===0?a:y===1?b:f(f(a)+f(f(b-a)*y));return y>=-2&&y<=3&&v>=f(limits.min)&&v<=f(limits.max);});
 if(i<0||i+1>=keys.length||keys[i].interpolation!=='cubic'||![1,2].includes(control)||!valid(curve,keys[i].value,keys[i+1].value))return [];
 const j=control===1?i-1:i+1;
 if(j>=0&&j+1<keys.length&&keys[j].interpolation==='cubic'){
  const other=round(keys[j].curve??DEFAULT_KEY_CURVE),dt=keys[i+1].frame-keys[i].frame,dv=keys[i+1].value-keys[i].value,nt=keys[j+1].frame-keys[j].frame,nv=keys[j+1].value-keys[j].value,minimum=f(.0001);
  if(!valid(other,keys[j].value,keys[j+1].value)||(!overshoot&&curveOvershoots(other))||dt<=0||nt<=0)return [];
  if(!preserveTime&&control===1){curve.x1=Math.max(curve.x1,minimum);curve.x2=Math.max(curve.x2,curve.x1);}else if(!preserveTime){curve.x2=f(1-Math.max(f(1-curve.x2),minimum));curve.x1=Math.min(curve.x1,curve.x2);}
  const sx=control===1?curve.x1:f(1-curve.x2),sy=control===1?curve.y1:f(1-curve.y2),bounds=curveValueBounds(keys[j].value,keys[j+1].value,f(limits.min),f(limits.max),overshoot),minX=control===1?f(1-f(1-minimum)):minimum,nyLow=control===1?1-bounds.max:bounds.min,nyHigh=control===1?1-bounds.min:bounds.max;
  if(sx<=0)return [];
  if(preserveTime){const ox=control===1?f(1-other.x2):other.x1,oy=control===1?f(1-other.y2):other.y1,desired=dv*sy/(dt*sx),existing=nv*oy/(nt*ox),precision=Math.abs(nv/nt)*2**-23*(Math.abs(control===1?other.y2:other.y1)/ox+Math.abs(oy)*Math.abs(control===1?other.x2:other.x1)/(ox*ox));if(ox>0&&Math.abs(desired-existing)<=1e-6*Math.max(Math.abs(desired),Math.abs(existing),1e-6)+2*precision)return [{index:i,curve}];}
  const requested=dv===0||nv===0?0:dv*sy/(dt*sx),r0=nv*nyLow/(nt*minX),r1=nv*nyHigh/(nt*minX),slope=Math.max(Math.min(r0,r1)*(1-1e-5),Math.min(Math.max(r0,r1)*(1-1e-5),requested)),sourceY=dv===0?0:slope*dt*sx/dv;
  if(!preserveTime||slope!==requested||dv===0||nv===0){if(control===1)curve.y1=f(sourceY);else curve.y2=f(1-sourceY);}
  const actualY=control===1?curve.y1:f(1-curve.y2),actualSlope=dv===0?0:dv*actualY/(dt*sx),ratio=nv===0?0:actualSlope*nt/nv,capacity=ratio<0?-nyLow:nyHigh,wanted=Math.max(minimum,control===1?f(1-other.x2):other.x1),nx=f(Math.max(minX,Math.min(wanted,ratio===0?1:capacity/Math.abs(ratio))));
  if(control===1){other.x2=f(1-nx);other.x1=Math.min(other.x1,other.x2);}else{other.x1=nx;other.x2=Math.max(other.x2,other.x1);}
  const actualX=control===1?f(1-other.x2):other.x1,ny=Math.max(nyLow,Math.min(nyHigh,ratio*actualX));
  if(control===1)other.y2=f(1-ny);else other.y1=f(ny);
  if(preserveTime&&ratio!==0){const encodedY=control===1?f(1-other.y2):other.y1,width=encodedY/ratio;if(width>=minX&&width<=1){if(control===1){other.x2=f(1-f(width));other.x1=Math.min(other.x1,other.x2);}else{other.x1=f(width);other.x2=Math.max(other.x2,other.x1);}}}
  if(!valid(curve,keys[i].value,keys[i+1].value)||!valid(other,keys[j].value,keys[j+1].value))return [];
  result.push({index:j,curve:other});
 }
 result.push({index:i,curve});return result;
}
