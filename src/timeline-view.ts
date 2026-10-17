// Display-only helpers. Authored timing and evaluated artwork remain in Rust.
export type TimelineKey = {frame:number;value:number;interpolation:'linear'|'hold'|'ease'};
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
  t=a.interpolation==='hold'?0:a.interpolation==='ease'?t*t*(3-2*t):t;
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
