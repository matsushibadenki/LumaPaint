import type {TimelineKey} from './timeline-view';
export const GRAPH={width:240,height:160,left:32,right:8,top:12,bottom:24};
export type GraphRange={min:number;max:number};
export function graphRange(keys:readonly TimelineKey[],base:number):GraphRange {
 let min=keys.length?Infinity:base,max=keys.length?-Infinity:base;
 for(const key of keys){min=Math.min(min,key.value);max=Math.max(max,key.value);}
 const pad=Math.max(1,(max-min)*.15);return {min:min-pad,max:max+pad};
}
export function graphPoint(frame:number,value:number,duration:number,range:GraphRange){return {x:GRAPH.left+frame/Math.max(1,duration-1)*(GRAPH.width-GRAPH.left-GRAPH.right),y:GRAPH.top+(range.max-value)/(range.max-range.min)*(GRAPH.height-GRAPH.top-GRAPH.bottom)};}
export function graphValue(y:number,range:GraphRange,min:number,max:number):number {
 const at=Math.max(GRAPH.top,Math.min(GRAPH.height-GRAPH.bottom,y));
 return Math.max(min,Math.min(max,Math.round((range.max-(at-GRAPH.top)/(GRAPH.height-GRAPH.top-GRAPH.bottom)*(range.max-range.min))*100)/100));
}
export function graphPath(keys:readonly TimelineKey[],duration:number,range:GraphRange,base:number):string {
 const f=(v:number)=>Number(v.toFixed(3)),point=(frame:number,value:number)=>{const p=graphPoint(frame,value,duration,range);return `${f(p.x)} ${f(p.y)}`;};
 if(!keys.length)return `M ${point(0,base)} L ${point(duration-1,base)}`;
 let path=`M ${point(0,keys[0].value)} L ${point(keys[0].frame,keys[0].value)}`;
 for(let i=1;i<keys.length;i++){const a=keys[i-1],b=keys[i];
  if(a.interpolation==='hold')path+=` L ${point(b.frame,a.value)} L ${point(b.frame,b.value)}`;
  else if(a.interpolation==='ease')path+=` C ${point(a.frame+(b.frame-a.frame)/3,a.value)} ${point(a.frame+2*(b.frame-a.frame)/3,b.value)} ${point(b.frame,b.value)}`;
  else path+=` L ${point(b.frame,b.value)}`;
 }
 return path+` L ${point(duration-1,keys.at(-1)!.value)}`;
}
export function graphMarkers(count:number,selected:number):number[]{
 const indices=new Set<number>();for(let i=0;i<Math.min(128,count);i++)indices.add(Math.round(i*(count-1)/Math.max(1,Math.min(128,count)-1)));
 if(selected>=0&&selected<count)indices.add(selected);return [...indices].sort((a,b)=>a-b);
}
