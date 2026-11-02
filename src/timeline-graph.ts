import type {TimelineKey} from './timeline-view';
export const GRAPH={width:240,height:160,left:32,right:8,top:12,bottom:24};
export type GraphRange={min:number;max:number};
export function graphRange(keys:readonly TimelineKey[],base:number):GraphRange {
 let min=keys.length?Infinity:base,max=keys.length?-Infinity:base;
 for(const key of keys){min=Math.min(min,key.value);max=Math.max(max,key.value);}
 const pad=Math.max(1,(max-min)*.15);return {min:min-pad,max:max+pad};
}
export function graphPoint(frame:number,value:number,duration:number,range:GraphRange,time={start:0,end:Math.max(1,duration-1)}){return {x:GRAPH.left+(frame-time.start)/Math.max(.000001,time.end-time.start)*(GRAPH.width-GRAPH.left-GRAPH.right),y:GRAPH.top+(range.max-value)/(range.max-range.min)*(GRAPH.height-GRAPH.top-GRAPH.bottom)};}
export function graphValue(y:number,range:GraphRange,min:number,max:number):number {
 const at=Math.max(GRAPH.top,Math.min(GRAPH.height-GRAPH.bottom,y));
 return Math.max(min,Math.min(max,Math.round((range.max-(at-GRAPH.top)/(GRAPH.height-GRAPH.top-GRAPH.bottom)*(range.max-range.min))*100)/100));
}
export function graphPath(keys:readonly TimelineKey[],duration:number,range:GraphRange,base:number,time={start:0,end:Math.max(1,duration-1)}):string {
 const f=(v:number)=>Number(v.toFixed(3)),point=(frame:number,value:number)=>{const p=graphPoint(frame,value,duration,range,time);return `${f(p.x)} ${f(p.y)}`;};
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
// Keys on the same property cannot share a frame. Crossing is allowed; choose
// the nearest free frame, preferring earlier ties (or keyboard direction).
export function graphFreeFrame(frame:number,from:number,keys:readonly TimelineKey[],duration:number,direction=0):number {
 const target=Math.max(0,Math.min(duration-1,Math.round(frame))),occupied=new Set(keys.filter(k=>k.frame!==from).map(k=>k.frame));
 for(let distance=0;distance<=keys.length;distance++)for(const candidate of direction>0?[target+distance,target-distance]:[target-distance,target+distance])if(candidate>=0&&candidate<duration&&!occupied.has(candidate))return candidate;
 return from;
}

export type KeyBatchEdit={kind:'move';frameDelta:number;valueDelta:number}|{kind:'shiftTime';delta:number}|{kind:'shiftValue';delta:number}|{kind:'interpolation';interpolation:TimelineKey['interpolation']}|{kind:'delete'};
export function graphBatch(keys:readonly TimelineKey[],frames:readonly number[],edit:KeyBatchEdit):TimelineKey[]{
 const selected=new Set(frames);
 return keys.filter(k=>edit.kind!=='delete'||!selected.has(k.frame)).map(k=>!selected.has(k.frame)?k:edit.kind==='move'?{...k,frame:k.frame+edit.frameDelta,value:Math.fround(Math.fround(k.value)+Math.fround(edit.valueDelta))}:edit.kind==='shiftTime'?{...k,frame:k.frame+edit.delta}:edit.kind==='shiftValue'?{...k,value:Math.fround(Math.fround(k.value)+Math.fround(edit.delta))}:edit.kind==='interpolation'?{...k,interpolation:edit.interpolation}:k).sort((a,b)=>a.frame-b.frame);
}
export function graphTimeDelta(raw:number,frames:readonly number[],keys:readonly TimelineKey[],duration:number,direction=0):number{
 if(!frames.length)return 0;
 const selected=new Set(frames),fixed=new Set(keys.filter(k=>!selected.has(k.frame)).map(k=>k.frame));
 const min=-Math.min(...frames),max=duration-1-Math.max(...frames),target=Math.max(min,Math.min(max,Math.round(raw)));
 for(let distance=0;distance<=max-min;distance++)for(const delta of direction>0?[target+distance,target-distance]:[target-distance,target+distance])if(delta>=min&&delta<=max&&frames.every(f=>!fixed.has(f+delta)))return delta;
 return 0;
}
export function graphValueDelta(raw:number,keys:readonly TimelineKey[],frames:readonly number[],min:number,max:number):number{
 const selected=new Set(frames),values=keys.filter(k=>selected.has(k.frame)).map(k=>k.value);
 return values.length?Math.max(min-Math.min(...values),Math.min(max-Math.max(...values),raw)):0;
}

export type GraphViewport=GraphRange&{start:number;end:number};
export type GraphAxis='both'|'time'|'value';
export function graphFullView(keys:readonly TimelineKey[],base:number,duration:number):GraphViewport{return {...graphRange(keys,base),start:0,end:Math.max(1,duration-1)};}
// View state never changes authored key values. Keep spans positive and bound
// navigation to the document and the property's editable output plus padding.
export function graphConstrainView(view:GraphViewport,duration:number,limits:GraphRange):GraphViewport{
 const timeMax=Math.max(1,duration-1),padding=Math.max(1,(limits.max-limits.min)*.01),low=limits.min-padding,high=limits.max+padding;
 const width=Math.max(Math.min(1,timeMax),Math.min(timeMax,view.end-view.start)),height=Math.max(.01,Math.min(high-low,view.max-view.min));
 const start=Math.max(0,Math.min(timeMax-width,view.start)),min=Math.max(low,Math.min(high-height,view.min));
 return {start,end:start+width,min,max:min+height};
}
export function graphZoomView(view:GraphViewport,factor:number,axis:GraphAxis,x:number,y:number,duration:number,limits:GraphRange):GraphViewport{
 const sx=Math.max(0,Math.min(1,x)),sy=Math.max(0,Math.min(1,y)),scale=Math.max(.1,Math.min(10,factor));
 const width=(view.end-view.start)/(axis==='value'?1:scale),height=(view.max-view.min)/(axis==='time'?1:scale),time=view.start+sx*(view.end-view.start),value=view.min+sy*(view.max-view.min);
 return graphConstrainView({start:time-sx*width,end:time+(1-sx)*width,min:value-sy*height,max:value+(1-sy)*height},duration,limits);
}
export function graphPanView(view:GraphViewport,dx:number,dy:number,duration:number,limits:GraphRange):GraphViewport{
 return graphConstrainView({...view,start:view.start+dx,end:view.end+dx,min:view.min+dy,max:view.max+dy},duration,limits);
}
export function graphFitView(keys:readonly TimelineKey[],base:number,duration:number,limits:GraphRange):GraphViewport{
 if(!keys.length)return graphFullView(keys,base,duration);
 const first=keys[0].frame,last=keys.at(-1)!.frame,pad=Math.max(1,(last-first)*.1);
 return graphConstrainView({...graphRange(keys,base),start:first-pad,end:last+pad},duration,limits);
}

// The app dispatcher runs in capture phase, before React's local handler.
// Reserve graph keys there while keeping global undo/save shortcuts available.
export function graphOwnsKey(key:string,primary:boolean,alt:boolean):boolean{
 if(primary)return !alt&&key.toLowerCase()==='a';
 if(alt)return key==='ArrowLeft'||key==='ArrowRight';
 return ['+','=','-','0','f','F','ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home','End','Delete','Backspace','Escape'].includes(key);
}

export function graphEditTimeDelta(edit:KeyBatchEdit):number{return edit.kind==='move'?edit.frameDelta:edit.kind==='shiftTime'?edit.delta:0;}
export function graphDragEdit(mode:'time'|'value'|'free',frameDelta:number,valueDelta:number):KeyBatchEdit{
 return mode==='free'?{kind:'move',frameDelta,valueDelta}:mode==='time'?{kind:'shiftTime',delta:frameDelta}:{kind:'shiftValue',delta:valueDelta};
}
