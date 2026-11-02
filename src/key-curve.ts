export type KeyCurve={x1:number;y1:number;x2:number;y2:number};
export const DEFAULT_KEY_CURVE:KeyCurve={x1:1/3,y1:0,x2:2/3,y2:1};
export function sampleKeyCurve(curve:KeyCurve,time:number):number{
 if(time<=0)return 0;if(time>=1)return 1;
 const bezier=(u:number,a:number,b:number)=>3*(1-u)**2*u*a+3*(1-u)*u*u*b+u*u*u;
 let low=0,high=1;for(let i=0;i<24;i++){const u=(low+high)/2;if(bezier(u,curve.x1,curve.x2)<time)low=u;else high=u;}
 return Math.max(0,Math.min(1,bezier((low+high)/2,curve.y1,curve.y2)));
}
export function curveControl(curve:KeyCurve,control:1|2,x:number,y:number):KeyCurve{
 const clamp=(n:number)=>Math.max(0,Math.min(1,n));
 return control===1?{...curve,x1:Math.min(curve.x2,clamp(x)),y1:clamp(y)}:{...curve,x2:Math.max(curve.x1,clamp(x)),y2:clamp(y)};
}
export function equalKeyCurve(a?:KeyCurve|null,b?:KeyCurve|null):boolean{
 return !a||!b?!a&&!b:(['x1','y1','x2','y2'] as const).every(k=>Math.fround(a[k])===Math.fround(b[k]));
}

// Same document-unit construction as Rust's atomic LinkedCurve edit.
export function linkedCurves(keys:readonly {frame:number;value:number;interpolation:string;curve?:KeyCurve|null}[],i:number,input:KeyCurve,control:1|2):{index:number;curve:KeyCurve}[]{
 const f=Math.fround,round=(c:KeyCurve):KeyCurve=>({x1:f(c.x1),y1:f(c.y1),x2:f(c.x2),y2:f(c.y2)});
 const curve=round(input);if(i<0||i+1>=keys.length)return [];
 const j=control===1?i-1:i+1,result=[];
 if(j>=0&&j+1<keys.length&&keys[j].interpolation==='cubic'){
  const other=round(keys[j].curve??DEFAULT_KEY_CURVE),dt=keys[i+1].frame-keys[i].frame,dv=keys[i+1].value-keys[i].value,nt=keys[j+1].frame-keys[j].frame,nv=keys[j+1].value-keys[j].value;
  const x=Math.max(f(.0001),control===1?curve.x1:f(1-curve.x2));let y=control===1?curve.y1:f(1-curve.y2);if(dv*nv<=0)y=0;
  if(control===1){curve.x1=x;curve.x2=Math.max(curve.x2,x);curve.y1=y;}else{curve.x2=f(1-x);curve.x1=Math.min(curve.x1,curve.x2);curve.y2=f(1-y);}
  const sx=control===1?curve.x1:f(1-curve.x2),sy=control===1?curve.y1:f(1-curve.y2),slope=dv*sy/(dt*sx),ratio=nv===0?0:Math.abs(slope*nt/nv),nx=f(Math.min(Math.max(f(.0001),control===1?f(1-other.x2):other.x1),ratio>0?1/ratio:1)),ny=f(Math.min(1,ratio*nx));
  if(control===1){other.x2=f(1-nx);other.x1=Math.min(other.x1,other.x2);other.y2=f(1-ny);}else{other.x1=nx;other.x2=Math.max(other.x2,nx);other.y1=ny;}
  result.push({index:j,curve:other});
 }
 result.push({index:i,curve});return result;
}
