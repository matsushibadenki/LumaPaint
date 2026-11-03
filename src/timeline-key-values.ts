import type {KeyProperty,KeyValueOffsets} from './timeline-key-group.ts';
import type {KeyTrack,LayerKey} from './timeline-key-tracks.ts';
export const KEY_VALUE_UNITS=['position','rotation','scale','opacity'] as const;
export type KeyValueUnit=typeof KEY_VALUE_UNITS[number];
export type KeyValueBounds=Record<KeyValueUnit,{min:number;max:number;count:number;values:number[]}>;
export function keyValueUnit(property:KeyProperty):KeyValueUnit{return property==='x'||property==='y'?'position':property==='rotation'?'rotation':property==='opacity'?'opacity':'scale';}
export function keyValueLimits(property:KeyProperty):[number,number]{return property==='opacity'?[0,100]:property==='scaleX'||property==='scaleY'?[.1,10000]:property==='rotation'?[-36000,36000]:[-65536,65536];}
export function selectedValueBounds(tracks:readonly KeyTrack[],selection:readonly LayerKey[]):KeyValueBounds{
 const fresh=()=>({min:-Infinity,max:Infinity,count:0,values:[] as number[]});
 const bounds:KeyValueBounds={position:fresh(),rotation:fresh(),scale:fresh(),opacity:fresh()};
 const indexed=new Map(tracks.map(t=>[t.id,t])),channels=new Map<string,Map<number,number>>();
 for(const key of selection){const id=JSON.stringify([key.layer,key.property]);let values=channels.get(id);if(!values){values=new Map((indexed.get(key.layer)?.channels[key.property]??[]).map(k=>[k.frame,k.value]));channels.set(id,values);}const value=values.get(key.frame);if(value===undefined)continue;const bound=bounds[keyValueUnit(key.property)],[min,max]=keyValueLimits(key.property);bound.min=Math.max(bound.min,min-value);bound.max=Math.min(bound.max,max-value);bound.count++;bound.values.push(Math.fround(value));}
 return bounds;
}
export function valueOffsetsValid(bounds:KeyValueBounds,offsets:KeyValueOffsets):boolean{
 return KEY_VALUE_UNITS.every(unit=>{const delta=Math.fround(offsets[unit]),bound=bounds[unit],[min,max]=keyValueLimits(({position:'x',rotation:'rotation',scale:'scaleX',opacity:'opacity'} as const)[unit]);return Number.isFinite(offsets[unit])&&Number.isFinite(delta)&&(!bound.count||offsets[unit]>=bound.min&&offsets[unit]<=bound.max&&bound.values.every(value=>{const next=Math.fround(value+delta);return next>=Math.fround(min)&&next<=Math.fround(max);}));});
}
export function valueOffsetsChanged(bounds:KeyValueBounds,offsets:KeyValueOffsets):boolean{return KEY_VALUE_UNITS.some(unit=>bounds[unit].values.some(value=>Math.fround(value+Math.fround(offsets[unit]))!==value));}
