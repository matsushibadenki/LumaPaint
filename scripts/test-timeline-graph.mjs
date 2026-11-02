import assert from 'node:assert/strict';
import {GRAPH,graphRange,graphPoint,graphValue,graphPath,graphMarkers} from '../src/timeline-graph.ts';
import {sampleTimelineKey} from '../src/timeline-view.ts';
for(const interpolation of ['linear','hold','ease']){
 const keys=[{frame:0,value:0,interpolation},{frame:10,value:100,interpolation:'linear'}];
 const range={min:0,max:100},path=graphPath(keys,11,range,0);
 if(interpolation==='ease'){
  const c=path.match(/C ([\d.]+) ([\d.]+) ([\d.]+) ([\d.]+) ([\d.]+) ([\d.]+)/).slice(1).map(Number),a=graphPoint(0,0,11,range);
  for(const frame of [1,2,5,8,9]){const t=frame/10,u=1-t;const y=u**3*a.y+3*u*u*t*c[1]+3*u*t*t*c[3]+t**3*c[5];const actual=(GRAPH.height-GRAPH.bottom-y)/(GRAPH.height-GRAPH.bottom-GRAPH.top)*100;assert.ok(Math.abs(actual-sampleTimelineKey(keys,frame,0))<.01);}
 }else if(interpolation==='hold')assert.ok(path.includes('L 232 136 L 232 12'));
 else assert.ok(path.includes('L 232 12'));
}
assert.deepEqual(graphRange([],100),{min:99,max:101});
assert.deepEqual(graphRange([{value:5}],0),{min:4,max:6});
for(const value of [-100,0,100]){const range={min:-100,max:100};assert.equal(graphValue(graphPoint(10,value,21,range).y,range,-100,100),value);}
assert.equal(graphValue(-999,{min:-20,max:120},0,100),100);
assert.equal(graphValue(999,{min:-20,max:120},0,100),0);
assert.ok(graphPath([],120,{min:-1,max:1},0).startsWith('M'));
for(const n of [0,1,2,128,4096]){const indices=graphMarkers(n,777);assert.ok(indices.length<=129);assert.equal(new Set(indices).size,indices.length);assert.ok(indices.every(i=>i>=0&&i<n));if(n>777)assert.ok(indices.includes(777));}
console.log('Value graph: exact easing geometry, hold/linear segments, axes, value limits, inverse mapping and bounded markers passed');
