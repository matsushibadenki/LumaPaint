import type {TimelineKey} from './timeline-view';
export const KEY_PROPERTIES=['x','y','scaleX','scaleY','rotation','opacity'] as const;
export type KeyProperty=typeof KEY_PROPERTIES[number];
export type PropertyKey={property:KeyProperty;frame:number};
export type KeyChannels=Partial<Record<KeyProperty,readonly TimelineKey[]>>;
export type KeyGroupEdit={kind:'shiftTime';delta:number}|{kind:'interpolation';interpolation:TimelineKey['interpolation']}|{kind:'delete'};
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
