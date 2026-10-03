// Keep major labels readable at every zoom; only visible ticks are constructed.
export function rulerTicks(length: number, origin: number, pixelsPerUnit: number) {
  if (!(length>0 && pixelsPerUnit>0) || !Number.isFinite(origin)) return [];
  const target=80/pixelsPerUnit, magnitude=10**Math.floor(Math.log10(target));
  const major=([1,2,5,10].find(n=>n*magnitude>=target)??10)*magnitude;
  const divisions=major/magnitude===2?4:5, minor=major/divisions;
  const start=Math.ceil(-origin/(pixelsPerUnit*minor));
  const end=Math.floor((length-origin)/(pixelsPerUnit*minor));
  const digits=Math.max(0,-Math.floor(Math.log10(major)));
  return Array.from({length:Math.min(2000,Math.max(0,end-start+1))},(_,i)=>{
    const index=start+i, value=index*minor;
    return {position:origin+value*pixelsPerUnit,major:index%divisions===0,label:(Math.abs(value)<minor/2?0:value).toFixed(digits)};
  });
}
