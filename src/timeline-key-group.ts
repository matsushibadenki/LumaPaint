import {timelineSnapTargets,type TimelineKey,type TimelineSnapTrack,type TimelineSnapFilters} from './timeline-view.ts';
export const KEY_PROPERTIES=['x','y','scaleX','scaleY','rotation','opacity'] as const;
export type KeyProperty=typeof KEY_PROPERTIES[number];
export type PropertyKey={property:KeyProperty;frame:number};
export type KeyChannels=Partial<Record<KeyProperty,readonly TimelineKey[]>>;
export type KeyValueOffsets={position:number;rotation:number;scale:number;opacity:number};
export type KeyGroupEdit=({kind:'shiftValues'}&KeyValueOffsets)|{kind:'shiftTime';delta:number}|{kind:'interpolation';interpolation:TimelineKey['interpolation']}|{kind:'delete'};
const same=(a:PropertyKey,b:PropertyKey)=>a.property===b.property&&a.frame===b.frame;
export function allPropertyKeys(channels:KeyChannels):PropertyKey[]{return KEY_PROPERTIES.flatMap(property=>(channels[property]??[]).map(k=>({property,frame:k.frame})));}
export function selectPropertyKeys(channels:KeyChannels,selection:readonly PropertyKey[],key:PropertyKey,anchor:PropertyKey|null,toggle:boolean,extend:boolean):PropertyKey[]{
 if(extend&&anchor){const a=KEY_PROPERTIES.indexOf(anchor.property),b=KEY_PROPERTIES.indexOf(key.property);return allPropertyKeys(channels).filter(k=>KEY_PROPERTIES.indexOf(k.property)>=Math.min(a,b)&&KEY_PROPERTIES.indexOf(k.property)<=Math.max(a,b)&&k.frame>=Math.min(anchor.frame,key.frame)&&k.frame<=Math.max(anchor.frame,key.frame)).slice(0,4096);}
 if(toggle)return selection.some(k=>same(k,key))?selection.filter(k=>!same(k,key)):selection.length<4096?[...selection,key]:[...selection];
 return [key];
}
export function propertyKeyGroups(selection:readonly PropertyKey[]){return KEY_PROPERTIES.map(property=>({property,frames:selection.filter(k=>k.property===property).map(k=>k.frame)})).filter(s=>s.frames.length);}
// A common collision-free offset preserves timing across all selected properties.
export function keyGroupDelta(raw:number,channels:KeyChannels,selection:readonly PropertyKey[],duration:number,direction=0):number{
 if(!selection.length)return 0;
 const groups=propertyKeyGroups(selection),frames=selection.map(k=>k.frame),min=-Math.min(...frames),max=duration-1-Math.max(...frames),target=Math.max(min,Math.min(max,Math.round(raw)));
 const occupied=groups.map(g=>{const picked=new Set(g.frames);return new Set((channels[g.property]??[]).filter(k=>!picked.has(k.frame)).map(k=>k.frame));});
 for(let d=0;d<=max-min;d++)for(const delta of direction>0?[target+d,target-d]:[target-d,target+d])if(delta>=min&&delta<=max&&groups.every((g,i)=>g.frames.every(f=>!occupied[i].has(f+delta))))return delta;
 return 0;
}

export function propertyKeysOwnKey(key:string,primary:boolean,alt:boolean):boolean{return !alt&&(primary?key.toLowerCase()==='a':['ArrowLeft','ArrowRight','Delete','Backspace','Escape'].includes(key));}

export type PropertyKeyDragPlan={targets:number[];edges:number[];min:number;max:number;groups:{frames:number[];occupied:Set<number>}[];free:Map<number,boolean>};
export function propertyKeyDragPlan(channels:KeyChannels,selection:readonly PropertyKey[],duration:number,targets:readonly number[]):PropertyKeyDragPlan{
 const frames=selection.map(k=>k.frame),groups=propertyKeyGroups(selection).map(g=>{const picked=new Set(g.frames);return {frames:g.frames,occupied:new Set((channels[g.property]??[]).filter(k=>!picked.has(k.frame)).map(k=>k.frame))};});
 return {targets:[...new Set(targets)].filter(Number.isFinite).sort((a,b)=>a-b),edges:[...new Set(frames)].sort((a,b)=>a-b),min:frames.length?-Math.min(...frames):0,max:frames.length?duration-1-Math.max(...frames):0,groups,free:new Map()};
}
export function propertyKeyDragDelta(raw:number,plan:PropertyKeyDragPlan,zoom:number,enabled:boolean,moved:boolean):{delta:number;snapped:boolean}{
 if(!moved||!Number.isFinite(raw))return {delta:0,snapped:false};
 const valid=(delta:number)=>{if(delta<plan.min||delta>plan.max)return false;let free=plan.free.get(delta);if(free===undefined){free=plan.groups.every(g=>g.frames.every(f=>!g.occupied.has(f+delta)));plan.free.set(delta,free);}return free;};
 if(enabled&&zoom>0){
  const candidates=new Set<number>(),radius=8/zoom;let index=0;
  for(const edge of plan.edges){const lo=raw+edge-radius,hi=raw+edge+radius;while(index<plan.targets.length&&plan.targets[index]<lo)index++;for(let i=index;i<plan.targets.length&&plan.targets[i]<=hi;i++){const delta=plan.targets[i]-edge;if(Number.isInteger(delta)&&delta>=plan.min&&delta<=plan.max)candidates.add(delta);}}
  for(const delta of [...candidates].sort((a,b)=>Math.abs(a-raw)-Math.abs(b-raw)||a-b))if(valid(delta))return {delta,snapped:true};
 }
 const target=Math.max(plan.min,Math.min(plan.max,Math.round(raw)));
 for(let d=0;d<=plan.max-plan.min;d++)for(const delta of [target-d,target+d])if(valid(delta))return {delta,snapped:false};
 return {delta:0,snapped:false};
}
export function visiblePropertyKeys(keys:readonly TimelineKey[],range:{from:number;to:number},selected:ReadonlySet<number>,delta:number,driver?:number):TimelineKey[]{
 return keys.filter(k=>k.frame===driver||((selected.has(k.frame)?k.frame+delta:k.frame)>=range.from&&(selected.has(k.frame)?k.frame+delta:k.frame)<range.to));
}

export function propertyKeySnapTargets(tracks:readonly TimelineSnapTrack[],layer:string,selection:readonly PropertyKey[],playhead:number,duration:number,filters:Readonly<TimelineSnapFilters>):number[]{
 const picked=new Set(selection.map(k=>`${k.property}:${k.frame}`));
 const filtered=tracks.map(l=>l.id===layer?{...l,channels:Object.fromEntries(Object.entries(l.channels).map(([property,keys])=>[property,keys?.filter(k=>!picked.has(`${property}:${k.frame}`))]))}:l);
 const targets=timelineSnapTargets(filtered,playhead,duration,{layer},filters);
 if(filters.bounds)targets.push(duration-1);
 return targets;
}
