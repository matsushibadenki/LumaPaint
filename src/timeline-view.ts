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
  const a=keys[right-1],b=keys[right];let t=(frame-a.frame)/(b.frame-a.frame);
  t=a.interpolation==='hold'?0:a.interpolation==='ease'?t*t*(3-2*t):t;
  return Math.round((a.value+(b.value-a.value)*t)*100)/100;
}
export function overlapsFrames(start:number,end:number,range:FrameRange):boolean {
  return start<range.to&&end>range.from;
}
