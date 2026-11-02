import {sampleKeyCurve,DEFAULT_KEY_CURVE,type KeyCurve} from './key-curve.ts';
// Display-only helpers. Authored timing and evaluated artwork remain in Rust.
export type TimelineKey = {frame:number;value:number;interpolation:'linear'|'hold'|'ease'|'cubic';curve?:KeyCurve};
export type FrameRange = {from:number;to:number};
export function visibleFrames(scrollLeft:number,width:number,zoom:number,duration:number):FrameRange {
  const from=Math.min(Math.max(0,duration-1),Math.max(0,Math.floor(scrollLeft/zoom)-12));
  const to=Math.min(duration,Math.max(from+1,Math.ceil((scrollLeft+Math.max(0,width-180))/zoom)+13));
  return {from,to};
}
function lowerBound(keys:readonly TimelineKey[],frame:number):number {
  let low=0,high=keys.length;
  while(low<high){const middle=(low+high)>>>1;if(keys[middle].frame<frame)low=middle+1;else high=middle;}
  return low;
}
export function visibleKeys(keys:readonly TimelineKey[],range:FrameRange,captured?:TimelineKey):TimelineKey[] {
  const result=keys.slice(lowerBound(keys,range.from),lowerBound(keys,range.to));
  if(captured&&(captured.frame<range.from||captured.frame>=range.to))result.push(captured);
  return result;
}
export function sampleTimelineKey(keys:readonly TimelineKey[],frame:number,base:number):number {
  if(!keys.length)return base;
  const right=lowerBound(keys,frame+1);
  if(right===0)return keys[0].value;
  if(right===keys.length)return keys.at(-1)!.value;
  const a=keys[right-1],b=keys[right];
  if(a.frame===frame)return a.value;
  let t=(frame-a.frame)/(b.frame-a.frame);
  t=a.interpolation==='hold'?0:a.interpolation==='ease'?t*t*(3-2*t):a.interpolation==='cubic'?sampleKeyCurve(a.curve??DEFAULT_KEY_CURVE,t):t;
  return Math.round((a.value+(b.value-a.value)*t)*100)/100;
}
export function overlapsFrames(start:number,end:number,range:FrameRange):boolean {
  return start<range.to&&end>range.from;
}

export type ExposureRange={id?:string;start:number;end:number;cel:number};
export type ExposureDragMode='move'|'start'|'end';
// Preview bounds only; Rust validates and commits the final interval atomically.
export function dragExposure(exposure:ExposureRange,delta:number,mode:ExposureDragMode,min:number,max:number):ExposureRange {
  delta=Math.round(delta);
  if(mode==='start')return {...exposure,start:Math.max(min,Math.min(exposure.end-1,exposure.start+delta))};
  if(mode==='end')return {...exposure,end:Math.max(exposure.start+1,Math.min(max,exposure.end+delta))};
  const start=Math.max(min,Math.min(max-(exposure.end-exposure.start),exposure.start+delta));
  return {...exposure,start,end:start+exposure.end-exposure.start};
}

export function exposureMoveBounds(exposures:readonly ExposureRange[],frames:readonly number[],duration:number,ripple:boolean):{min:number;max:number}{
 if(!frames.length)return {min:0,max:0};
 const selected=new Set(frames);const first=Math.min(...frames);const moved=exposures.filter(e=>selected.has(e.start)||ripple&&e.start>=first);
 if(!moved.length)return {min:0,max:0};let min=0-moved[0].start,max=(ripple?18000:duration)-moved.at(-1)!.end;
 const fixed=exposures.filter(e=>!selected.has(e.start)&&!(ripple&&e.start>=first));
 const bound=(value:number,end:boolean)=>{let lo=0,hi=fixed.length;while(lo<hi){const mid=(lo+hi)>>>1;const before=end?fixed[mid].end<=value:fixed[mid].start<value;if(before)lo=mid+1;else hi=mid;}return lo;};
 for(const e of moved){const previous=fixed[bound(e.start,true)-1],next=fixed[bound(e.end,false)];if(previous)min=Math.max(min,previous.end-e.start);if(next)max=Math.min(max,next.start-e.end);}
 return {min,max};
}
export function selectExposureFrames(exposures:readonly ExposureRange[],selected:readonly number[],frame:number,anchor:number|null,range:boolean,toggle:boolean):number[]{
 if(range&&anchor!==null){const a=exposures.findIndex(e=>e.start===anchor),b=exposures.findIndex(e=>e.start===frame);if(a>=0&&b>=0)return exposures.slice(Math.min(a,b),Math.max(a,b)+1).map(e=>e.start);}
 if(toggle)return (selected.includes(frame)?selected.filter(f=>f!==frame):[...selected,frame]).sort((a,b)=>a-b);
 return [frame];
}

// Pixels per second, bounded and independent of display refresh rate.
export function timelineEdgeScrollSpeed(x:number,left:number,right:number):number {
 if(!Number.isFinite(x)||right<=left)return 0;
 const edge=Math.min(40,(right-left)/3);
 if(x<left+edge)return -720*Math.min(1,Math.max(0,(left+edge-x)/edge));
 if(x>right-edge)return 720*Math.min(1,Math.max(0,(x-right+edge)/edge));
 return 0;
}

export function selectedExposurePositions(exposures:readonly ExposureRange[],ids:readonly string[],anchor:string|null):{frames:number[];anchor:number|null} {
 const selected=new Set(ids);let anchorFrame:number|null=null;
 const frames=exposures.filter(e=>e.id!==undefined&&selected.has(e.id)).map(e=>{if(e.id===anchor)anchorFrame=e.start;return e.start;}).sort((a,b)=>a-b);
 return {frames,anchor:anchorFrame??frames[0]??null};
}

export function exposureDragGesture(pointerPixels:number,scrollPixels:number,timingChanged=false):boolean {
 return timingChanged||Math.abs(pointerPixels)>3||Math.abs(scrollPixels)>3;
}

// Resolve a displayed insertion boundary; Rust owns the reordered timing plan.
export function exposureReorderTarget(exposures:readonly ExposureRange[],frames:readonly number[],frame:number):{before:number|null;frame:number}|null {
 if(!Number.isFinite(frame)||!frames.length)return null;
 const selected=new Set(frames);const fixed=exposures.filter(e=>!selected.has(e.start));
 if(!fixed.length)return null;
 const target=fixed.find(e=>frame<(e.start+e.end)/2);
 return target?{before:target.start,frame:target.start}:{before:null,frame:exposures.at(-1)!.end};
}

export type ExposureTrack={id:string;pixel:boolean;locked:boolean;exposures:readonly ExposureRange[]};
export type ExposureSelection={tracks:Record<string,number[]>;anchor:{layer:string;frame:number}|null};
export function selectExposureTracks(tracks:readonly ExposureTrack[],selection:ExposureSelection,layer:string,frame:number,range:boolean,toggle:boolean):ExposureSelection {
 const target=tracks.find(t=>t.id===layer);if(!target?.pixel||target.locked||!target.exposures.some(e=>e.start===frame))return selection;
 const anchor=selection.anchor;
 if(range&&anchor){const a=tracks.findIndex(t=>t.id===anchor.layer),b=tracks.findIndex(t=>t.id===layer);if(a>=0&&b>=0){const first=Math.min(anchor.frame,frame),last=Math.max(anchor.frame,frame);const next:Record<string,number[]>=Object.create(null);for(const track of tracks.slice(Math.min(a,b),Math.max(a,b)+1)){if(!track.pixel||track.locked)continue;const frames=track.exposures.filter(e=>e.start<=last&&e.end>first).map(e=>e.start);if(frames.length)next[track.id]=frames;}return {tracks:Object.fromEntries(Object.entries(next)),anchor};}}
 if(toggle){const selected=Object.hasOwn(selection.tracks,layer)?selection.tracks[layer]:[];const frames=selectExposureFrames(target.exposures,selected,frame,null,false,true);const next:Record<string,number[]>=Object.assign(Object.create(null),selection.tracks);if(frames.length)next[layer]=frames;else delete next[layer];return {tracks:Object.fromEntries(Object.entries(next)),anchor:{layer,frame}};}
 return {tracks:{[layer]:[frame]},anchor:{layer,frame}};
}
export function exposureGroupMoveBounds(tracks:readonly ExposureTrack[],selection:Record<string,readonly number[]>,duration:number,ripple:boolean):{min:number;max:number}{
 let min=-18000,max=18000,found=false;
 for(const track of tracks){const frames=Object.hasOwn(selection,track.id)?selection[track.id]:undefined;if(!frames?.length)continue;if(!track.pixel||track.locked)return {min:0,max:0};const bounds=exposureMoveBounds(track.exposures,frames,duration,ripple);min=Math.max(min,bounds.min);max=Math.min(max,bounds.max);found=true;}
 return found?{min,max}:{min:0,max:0};
}

// A shared absolute hold length matches Rust ExposureBatchEdit::Hold.
export function exposureGroupHoldBounds(tracks:readonly ExposureTrack[],selection:Record<string,readonly number[]>,duration:number,ripple:boolean):{min:number;max:number}{
 let max=18000,found=false;
 for(const track of tracks){
  const frames=selection[track.id];if(!frames?.length)continue;
  if(!track.pixel||track.locked)return {min:1,max:1};
  const selected=new Set(frames);let count=0,oldLength=0;
  for(let i=0;i<track.exposures.length;i++){const e=track.exposures[i];if(!selected.has(e.start))continue;found=true;count++;oldLength+=e.end-e.start;
   if(!ripple)max=Math.min(max,(track.exposures[i+1]?.start??duration)-e.start);
  }
  if(ripple&&count)max=Math.min(max,Math.floor((18000-(track.exposures.at(-1)?.end??0)+oldLength)/count));
 }
 return {min:1,max:found?Math.max(1,max):1};
}
export function exposureGroupHoldPreview(tracks:readonly ExposureTrack[],selection:Record<string,readonly number[]>,length:number,ripple:boolean):Map<string,Map<number,ExposureRange>>{
 const result=new Map<string,Map<number,ExposureRange>>();
 for(const track of tracks){const frames=selection[track.id];if(!frames?.length)continue;const selected=new Set(frames),ranges=new Map<number,ExposureRange>();let offset=0;
  for(const e of track.exposures){const hold=selected.has(e.start)?length:e.end-e.start;ranges.set(e.start,{...e,start:e.start+offset,end:e.start+offset+hold});if(ripple)offset+=hold-(e.end-e.start);}
  result.set(track.id,ranges);
 }
 return result;
}

// Display preview uses the same frame bounds as the motion command.
export function motionDragStart(start:number,duration:number,repeats:number,pointerDelta:number,scrollDelta:number,zoom:number):number {
 return Math.max(0,Math.min(Math.max(0,18000-duration*repeats),start+Math.round((pointerDelta+scrollDelta)/zoom)));
}

// Sorted frame targets are captured once per gesture; authored edits remain in Rust.
export type TimelineSnapFilters={playhead:boolean;keys:boolean;exposures:boolean;motions:boolean;bounds:boolean};
export const defaultTimelineSnapFilters:Readonly<TimelineSnapFilters>=Object.freeze({playhead:true,keys:true,exposures:true,motions:true,bounds:true});
export type TimelineSnapTrack={id:string;channels:Partial<Record<string,readonly TimelineKey[]>>;exposures:readonly ExposureRange[];motions:readonly {id:string;start:number;duration:number;repeats:number}[]};
export function timelineSnapTargets(tracks:readonly TimelineSnapTrack[],playhead:number,duration:number,moving:{layer:string;property?:string;frame?:number;clip?:string;span?:number},filters:Readonly<TimelineSnapFilters>=defaultTimelineSnapFilters):number[] {
 const targets=new Set<number>(filters.bounds?[0,duration]:[]);if(filters.playhead)targets.add(playhead);
 const blocked=new Set<number>();
 for(const track of tracks){
  for(const [property,keys] of Object.entries(track.channels))for(const key of keys??[]){
   const same=track.id===moving.layer&&property===moving.property;
   if(same&&key.frame===moving.frame)continue;
   if(same)blocked.add(key.frame);
   if(filters.keys)targets.add(key.frame);
  }
  if(filters.exposures)for(const cel of track.exposures){targets.add(cel.start);targets.add(cel.end);}
  if(filters.motions)for(const clip of track.motions){if(track.id===moving.layer&&clip.id===moving.clip)continue;targets.add(clip.start);targets.add(clip.start+clip.duration*clip.repeats);}
 }
 // A clip can align either edge; the returned value is always its new start.
 if(moving.span)for(const target of [...targets])targets.add(target-moving.span);
 return [...targets].filter(frame=>Number.isFinite(frame)&&!blocked.has(frame)).sort((a,b)=>a-b);
}
export function snapTimelineFrame(raw:number,targets:number|readonly number[],zoom:number,min:number,max:number,enabled:boolean,moved:boolean):{frame:number;snapped:boolean} {
 const values=typeof targets==='number'?[targets]:targets;
 const query=Math.max(min,Math.min(max,raw));
 let low=0,high=values.length;
 while(low<high){const middle=(low+high)>>>1;if(values[middle]<query)low=middle+1;else high=middle;}
 let target:number|undefined;
 for(const index of [low-1,low]){const value=values[index];if(Number.isFinite(value)&&value>=min&&value<=max&&(target===undefined||Math.abs(raw-value)<Math.abs(raw-target)))target=value;}
 const snapped=enabled&&moved&&Number.isFinite(raw)&&target!==undefined&&zoom>0&&Math.abs(raw-target)*zoom<=8;
 return {frame:Math.max(min,Math.min(max,Math.round(snapped?target!:raw))),snapped};
}

// UI geometry only. Timing, selection and authored state remain in Rust.
export const TIMELINE_RULER_HEIGHT = 26;
export const TIMELINE_MOTION_ROW_HEIGHT = 38;
// Uniform rows need no per-clip layout allocation or full-list scan on scroll.
export function visibleTimelineMotionRows(count:number,top:number,height:number,retained:readonly number[]=[],overscan=76):number[]{
 const start=Math.max(0,Math.min(count,Math.floor((top-overscan)/TIMELINE_MOTION_ROW_HEIGHT)));
 const end=Math.max(start,Math.min(count,Math.ceil((top+Math.max(0,height)+overscan)/TIMELINE_MOTION_ROW_HEIGHT)));
 const rows=new Set<number>();for(let i=start;i<end;i++)rows.add(i);
 for(const i of retained)if(Number.isInteger(i)&&i>=0&&i<count)rows.add(i);
 return [...rows].sort((a,b)=>a-b);
}
export function timelineLayerHeight(pixel:boolean,folded:boolean,thumbnailSize:number,motions:number,selected:boolean):number {
  if(folded)return 30;
  return (pixel&&thumbnailSize?Math.max(44,thumbnailSize+26):44)+(pixel?0:motions*TIMELINE_MOTION_ROW_HEIGHT+(selected?6*28:0));
}
export function timelineLayerLayout<T extends {id:string}>(layers:readonly T[],height:(layer:T)=>number):{rows:Array<{item:T;top:number;height:number}>;height:number} {
  let top=TIMELINE_RULER_HEIGHT;
  const rows=layers.map(item=>{const size=height(item);const row={item,top,height:size};top+=size;return row;});
  return {rows,height:top};
}
export function visibleTimelineLayers<T extends {id:string}>(rows:readonly {item:T;top:number;height:number}[],top:number,height:number,retained:readonly (string|null|undefined)[]=[],overscan=160):number[] {
  const start=Math.max(0,top-overscan),end=top+Math.max(0,height)+overscan;
  let low=0,high=rows.length;
  while(low<high){const middle=(low+high)>>>1;if(rows[middle].top+rows[middle].height<=start)low=middle+1;else high=middle;}
  const indices=new Set<number>();
  for(let i=low;i<rows.length&&rows[i].top<end;i++)indices.add(i);
  const keep=new Set(retained.filter(Boolean));
  if(keep.size)rows.forEach((row,index)=>{if(keep.has(row.item.id))indices.add(index);});
  return [...indices].sort((a,b)=>a-b);
}

export type ExposureSnapPlan={targets:number[];edges:number[]};
// Moving group members (and ripple followers) must never attract each other.
export function exposureSnapPlan(tracks:readonly TimelineSnapTrack[],playhead:number,duration:number,selection:Readonly<Record<string,readonly number[]>>,driver:{layer:string;start:number;end:number},mode:ExposureDragMode,ripple:boolean,filters:Readonly<TimelineSnapFilters>=defaultTimelineSnapFilters):ExposureSnapPlan {
 const targets=new Set<number>(filters.bounds?[0,duration]:[]),edges=new Set<number>();if(filters.playhead)targets.add(playhead);
 const chosen=mode!=='start'&&selection[driver.layer]?.includes(driver.start)?selection:{[driver.layer]:[driver.start]};
 for(const track of tracks){
  if(filters.keys)for(const keys of Object.values(track.channels))for(const key of keys??[])targets.add(key.frame);
  if(filters.motions)for(const clip of track.motions){targets.add(clip.start);targets.add(clip.start+clip.duration*clip.repeats);}
  const selected=new Set(chosen[track.id]??[]);let first=Infinity;for(const frame of selected)first=Math.min(first,frame);
  for(const cel of track.exposures){
   const source=track.id===driver.layer&&cel.start===driver.start;
   const moving=mode==='move'&&(selected.has(cel.start)||ripple&&cel.start>=first);
   const follower=mode==='end'&&ripple&&track.id===driver.layer&&cel.start>driver.start;
   if(moving){if(selected.has(cel.start)){edges.add(cel.start);edges.add(cel.end);}}
   else if(follower){/* Ripple followers move, but are not explicit alignment edges. */}
   else if(source)edges.add(mode==='start'?cel.start:cel.end);
   else if(mode==='end'&&selected.has(cel.start)){if(filters.exposures)targets.add(cel.start);}
   else if(filters.exposures){targets.add(cel.start);targets.add(cel.end);}
  }
 }
 const sorted=(set:Set<number>)=>[...set].filter(Number.isFinite).sort((a,b)=>a-b);
 return {targets:sorted(targets),edges:sorted(edges)};
}
// One common delta preserves every selected interval and its relative timing.
// Both lists are sorted at gesture start; the nearest-pair scan is linear.
export function snapExposureDelta(raw:number,plan:ExposureSnapPlan,zoom:number,min:number,max:number,enabled:boolean,moved:boolean):{delta:number;snapped:boolean} {
 const base={delta:Math.max(min,Math.min(max,Math.round(Number.isFinite(raw)?raw:0))),snapped:false};
 if(!enabled||!moved||!Number.isFinite(raw)||zoom<=0)return base;
 let index=0,best:number|undefined,distance=Infinity;
 for(const edge of plan.edges){
  const query=Math.max(min,Math.min(max,raw))+edge;
  while(index<plan.targets.length&&plan.targets[index]<query)index++;
  for(const i of [index-1,index]){
   const delta=plan.targets[i]-edge,error=Math.abs(delta-raw);
   if(delta>=min&&delta<=max&&error*zoom<=8&&(error<distance||error===distance&&(best===undefined||delta<best))){best=delta;distance=error;}
  }
 }
 return best===undefined?base:{delta:best,snapped:true};
}
