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

export type ExposureRange={start:number;end:number;cel:number};
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
 if(!moved.length)return {min:0,max:0};let min=-moved[0].start,max=(ripple?18000:duration)-moved.at(-1)!.end;
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
