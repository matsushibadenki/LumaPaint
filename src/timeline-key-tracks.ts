import {timelineSnapTargets,type TimelineSnapTrack,type TimelineSnapFilters} from './timeline-view.ts';
import {KEY_PROPERTIES,propertyKeyGroups,propertyKeyDragPlan,type PropertyKey,type KeyChannels,type PropertyKeyDragPlan} from './timeline-key-group.ts';
export type LayerKey=PropertyKey&{layer:string};
export type KeyTrack={id:string;pixel:boolean;locked:boolean;channels:KeyChannels};
export const layerKeyId=(key:LayerKey)=>JSON.stringify([key.layer,key.property,key.frame]);
function* editableKeys(tracks:readonly KeyTrack[]):Generator<LayerKey>{for(const l of tracks)if(!l.pixel&&!l.locked)for(const property of KEY_PROPERTIES)for(const key of l.channels[property]??[])yield {layer:l.id,property,frame:key.frame};}
export function allLayerKeys(tracks:readonly KeyTrack[]):LayerKey[]{const result:LayerKey[]=[];for(const key of editableKeys(tracks)){result.push(key);if(result.length===4096)break;}return result;}
export function layerKeyGroups(selected:readonly LayerKey[]){const groups=new Map<string,LayerKey[]>();for(const k of selected){const list=groups.get(k.layer)??[];list.push(k);groups.set(k.layer,list);}return [...groups].map(([layer,keys])=>({layer,selections:propertyKeyGroups(keys)}));}
export function selectLayerKeys(tracks:readonly KeyTrack[],selected:readonly LayerKey[],key:LayerKey,anchor:LayerKey|null,toggle:boolean,extend:boolean):LayerKey[]{
 if(extend&&anchor){const rows=new Map(tracks.filter(t=>!t.pixel).flatMap(t=>KEY_PROPERTIES.map(p=>JSON.stringify([t.id,p]))).map((r,i)=>[r,i])),a=rows.get(JSON.stringify([anchor.layer,anchor.property])),b=rows.get(JSON.stringify([key.layer,key.property]));if(a!==undefined&&b!==undefined){const result:LayerKey[]=[];for(const k of editableKeys(tracks)){const row=rows.get(JSON.stringify([k.layer,k.property]))!;if(row>=Math.min(a,b)&&row<=Math.max(a,b)&&k.frame>=Math.min(anchor.frame,key.frame)&&k.frame<=Math.max(anchor.frame,key.frame)){result.push(k);if(result.length===4096)break;}}return result;}}

 const id=layerKeyId(key);if(toggle)return selected.some(k=>layerKeyId(k)===id)?selected.filter(k=>layerKeyId(k)!==id):selected.length<4096?[...selected,key]:[...selected];return [key];
}
export function layerKeyDragPlan(tracks:readonly KeyTrack[],selected:readonly LayerKey[],duration:number,targets:readonly number[]):PropertyKeyDragPlan{
 const indexed=new Map(tracks.map(t=>[t.id,t])),frames=selected.map(k=>k.frame),groups=layerKeyGroups(selected).flatMap(t=>propertyKeyDragPlan(indexed.get(t.layer)?.channels??{},t.selections.flatMap(s=>s.frames.map(frame=>({property:s.property,frame}))),duration,[]).groups);
 return {targets:[...new Set(targets)].sort((a,b)=>a-b),edges:[...new Set(frames)].sort((a,b)=>a-b),min:frames.length?-Math.min(...frames):0,max:frames.length?duration-1-Math.max(...frames):0,groups,free:new Map()};
}
export function layerKeyDelta(raw:number,plan:PropertyKeyDragPlan,direction=0):number{
 const target=Math.max(plan.min,Math.min(plan.max,Math.round(raw)));
 for(let d=0;d<=plan.max-plan.min;d++)for(const delta of direction>0?[target+d,target-d]:[target-d,target+d])if(delta>=plan.min&&delta<=plan.max&&plan.groups.every(g=>g.frames.every(f=>!g.occupied.has(f+delta))))return delta;return 0;
}
export function layerKeySnapTargets(tracks:readonly TimelineSnapTrack[],selected:readonly LayerKey[],playhead:number,duration:number,filters:Readonly<TimelineSnapFilters>):number[]{
 const picked=new Set(selected.map(layerKeyId)),filtered=tracks.map(l=>({...l,channels:Object.fromEntries(Object.entries(l.channels).map(([property,keys])=>[property,keys?.filter(k=>!picked.has(layerKeyId({layer:l.id,property:property as LayerKey['property'],frame:k.frame})))]))}));
 const targets=timelineSnapTargets(filtered,playhead,duration,{layer:''},filters);if(filters.bounds)targets.push(duration-1);return targets;
}
