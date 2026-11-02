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
