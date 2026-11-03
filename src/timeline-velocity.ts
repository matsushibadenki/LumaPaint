// Derivatives of the authored property curves, in units / second.
import type {TimelineKey} from './timeline-view';
import {DEFAULT_KEY_CURVE,curveOvershoots,curveValueBounds,type KeyCurve} from './key-curve.ts';
export type VelocityPoint={frame:number;value:number};
export type VelocityData={lines:VelocityPoint[][];jumps:number[];singular:number[];min:number;max:number};
const cubic=(u:number,a:number,b:number)=>3*(1-u)**2*u*a+3*(1-u)*u*u*b+u*u*u;
export function curveSlope(curve:KeyCurve,u:number):number|null{
 const derivative=(a:number,b:number)=>3*(1-u)**2*a+6*(1-u)*u*(b-a)+3*u*u*(1-b);
 const dx=derivative(curve.x1,curve.x2),dy=derivative(curve.y1,curve.y2);
 if(dx>0)return dy/dx;
 // At a stationary endpoint use the first nonzero polynomial coefficient.
 const c=u===1?{x1:1-curve.x2,x2:1-curve.x1,y1:1-curve.y2,y2:1-curve.y1}:curve;
 const xs=[c.x1,2*(c.x2-2*c.x1),1-3*c.x2+3*c.x1],ys=[c.y1,2*(c.y2-2*c.y1),1-3*c.y2+3*c.y1];
 const xi=xs.findIndex(n=>Math.abs(n)>1e-12),yi=ys.findIndex(n=>Math.abs(n)>1e-12);
 return yi<0||yi>xi?0:yi<xi||xi<0?null:ys[yi]/xs[xi];
}
export function velocityData(keys:readonly TimelineKey[],fps:number,duration:number,start=0,end=Math.max(1,duration-1)):VelocityData{
 const lines:VelocityPoint[][]=[],jumps:number[]=[],singular:number[]=[];
 let min=0,max=0;
 const add=(line:VelocityPoint[])=>{if(!line.length)return;lines.push(line);for(const p of line){min=Math.min(min,p.value);max=Math.max(max,p.value);}};
 const zero=(a:number,b:number)=>{if(b>=start&&a<=end)add([{frame:Math.max(a,start),value:0},{frame:Math.min(b,end),value:0}]);};
 if(!keys.length)zero(0,Math.max(1,duration-1));
 else{
  zero(0,keys[0].frame);zero(keys.at(-1)!.frame,Math.max(1,duration-1));
  const intervals=keys.slice(0,-1).map((a,i)=>({a,b:keys[i+1]})).filter(({a,b})=>b.frame>=start&&a.frame<=end);
  const steps=Math.max(2,Math.min(64,Math.floor(2048/Math.max(1,intervals.length))));
  for(const {a,b} of intervals){
   const dt=b.frame-a.frame,rate=(b.value-a.value)/dt*fps;
   if(a.interpolation==='hold'){zero(a.frame,b.frame);if(a.value!==b.value&&b.frame>=start&&b.frame<=end)jumps.push(b.frame);continue;}
   if(a.interpolation==='linear'||rate===0){add([{frame:Math.max(a.frame,start),value:rate},{frame:Math.min(b.frame,end),value:rate}]);continue;}
   const curve=a.curve??DEFAULT_KEY_CURVE,line:VelocityPoint[]=[],custom=a.interpolation==='cubic';
   const parameter=(time:number)=>{let low=0,high=1;for(let i=0;i<24;i++){const u=(low+high)/2;if(cubic(u,curve.x1,curve.x2)<time)low=u;else high=u;}return time<=0?0:time>=1?1:(low+high)/2;};
   const t0=Math.max(0,(start-a.frame)/dt),t1=Math.min(1,(end-a.frame)/dt),u0=custom?parameter(t0):t0,u1=custom?parameter(t1):t1;
   for(let i=0;i<=steps;i++){
    const u=u0+(u1-u0)*i/steps,time=custom?cubic(u,curve.x1,curve.x2):u,slope=custom?curveSlope(curve,u):6*u*(1-u),frame=a.frame+dt*time;
    if(slope===null){if(frame>=start&&frame<=end)singular.push(frame);continue;}
    const value=rate*slope;if(Number.isFinite(value))line.push({frame,value});
   }
   add(line);
  }
 }
 // Zero belongs in the range so direction and stopped portions remain legible.
 const pad=Math.max(.01,(max-min)*.12);return {lines,jumps,singular,min:min-pad,max:max+pad};
}
export function velocityAt(keys:readonly TimelineKey[],fps:number,frame:number):number|null{
 if(!keys.length||frame<keys[0].frame||frame>keys.at(-1)!.frame)return 0;
 let i=keys.findIndex(k=>k.frame>=frame);
 if(i>=0&&keys[i].frame===frame){
  const left=i===0?0:intervalVelocity(keys[i-1],keys[i],fps,1),right=i===keys.length-1?0:intervalVelocity(keys[i],keys[i+1],fps,0);
  if(i>0&&keys[i-1].interpolation==='hold'&&keys[i-1].value!==keys[i].value)return null;
  return left===null||right===null||Math.abs(left-right)>1e-6*Math.max(1,Math.abs(left),Math.abs(right))?null:right;
 }
 if(i<0)return 0;const a=keys[i-1],b=keys[i];return intervalVelocity(a,b,fps,(frame-a.frame)/(b.frame-a.frame));
}
function intervalVelocity(a:TimelineKey,b:TimelineKey,fps:number,time:number):number|null{
 const rate=(b.value-a.value)/(b.frame-a.frame)*fps;
 if(rate===0||a.interpolation==='hold')return 0;if(a.interpolation==='linear')return rate;
 if(a.interpolation==='ease')return rate*6*time*(1-time);
 const c=a.curve??DEFAULT_KEY_CURVE;let low=0,high=1;
 for(let i=0;i<24;i++){const u=(low+high)/2;if(cubic(u,c.x1,c.x2)<time)low=u;else high=u;}
 const slope=curveSlope(c,time===0?0:time===1?1:(low+high)/2);return slope===null?null:rate*slope;
}

// Mirrors Rust's Velocity edit; returned handles are the authored float32 values.
export function velocityCurve(a:TimelineKey,b:TimelineKey,fps:number,control:1|2,value:number,limits={min:-65536,max:65536},overshoot=false):KeyCurve|null{
 const info=velocityControl(a,b,fps,control,limits,overshoot);if(!info||!Number.isFinite(value)||value<info.min||value>info.max)return null;
 const c={...info.curve};if(value===info.value)return c;
 const y=control===1?value/info.rate*info.x:1-value/info.rate*info.x;
 if(control===1)c.y1=Math.fround(y);else c.y2=Math.fround(y);return c;
}
export function velocityControl(a:TimelineKey|undefined,b:TimelineKey|undefined,fps:number,control:1|2,limits={min:-65536,max:65536},overshoot=false){
 if(!a||!b||b.frame<=a.frame||!Number.isFinite(fps)||fps<=0||a.interpolation==='hold')return null;
 const f=Math.fround,c=a.interpolation==='linear'?{x1:f(1/3),y1:f(1/3),x2:f(2/3),y2:f(2/3)}:a.interpolation==='ease'?{x1:f(1/3),y1:0,x2:f(2/3),y2:1}:a.curve??{x1:f(1/3),y1:0,x2:f(2/3),y2:1};
 const x=control===1?c.x1:1-c.x2,rate=(b.value-a.value)*fps/(b.frame-a.frame);
 if((!overshoot&&curveOvershoots(c))||x<=0||rate===0||!Number.isFinite(rate))return null;
 const value=rate*(control===1?c.y1:1-c.y2)/x,bounds=curveValueBounds(a.value,b.value,f(limits.min),f(limits.max),overshoot),rates=control===1?[rate*bounds.min/x,rate*bounds.max/x]:[rate*(1-bounds.max)/x,rate*(1-bounds.min)/x];
 // Include an existing valid boundary handle without quantizing it again.
 return {curve:c,x,rate,value,min:Math.min(value,...rates),max:Math.max(value,...rates)};
}
