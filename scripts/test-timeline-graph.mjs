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

const {graphFreeFrame}=await import('../src/timeline-graph.ts');
for(const duration of [1,2,12,120])for(const from of [0,Math.floor(duration/2),duration-1]){
 const keys=Array.from({length:duration},(_,frame)=>({frame,value:frame,interpolation:'linear'})).filter(k=>k.frame===from||k.frame%3===0);
 for(let raw=-1;raw<=duration;raw++)for(const direction of [-1,0,1]){
  const actual=graphFreeFrame(raw,from,keys,duration,direction),target=Math.max(0,Math.min(duration-1,Math.round(raw)));
  const occupied=new Set(keys.filter(k=>k.frame!==from).map(k=>k.frame));
  const candidates=Array.from({length:duration},(_,i)=>i).filter(i=>!occupied.has(i)).sort((a,b)=>Math.abs(a-target)-Math.abs(b-target)||(direction>0?b-a:a-b));
  assert.equal(actual,candidates[0]);
 }
}
assert.equal(graphFreeFrame(30,5,[{frame:5},{frame:30}],120),29);
assert.equal(graphFreeFrame(30,5,[{frame:5},{frame:30}],120,1),31);
assert.equal(graphFreeFrame(40,5,[{frame:5},{frame:30}],120),40);
assert.equal(graphFreeFrame(2,0,[{frame:0},{frame:1},{frame:2}],3),0);
console.log('Graph timing: collision-free nearest frame, crossing, boundary/one-frame/full occupancy and keyboard direction passed');

const {graphBatch,graphTimeDelta,graphValueDelta}=await import('../src/timeline-graph.ts');
const group=[{frame:5,value:10,interpolation:'ease'},{frame:15,value:30,interpolation:'linear'},{frame:30,value:80,interpolation:'hold'}];
assert.equal(graphTimeDelta(10,[5,15],group,120),10); // selected source positions can be reused
assert.equal(graphTimeDelta(15,[5,15],group,120),14); // fixed key blocks a common delta
assert.equal(graphTimeDelta(999,[5,15],group,120),104);
assert.equal(graphTimeDelta(-999,[5,15],group,120),-5);
assert.equal(graphValueDelta(100,group,[5,15],0,100),70);
assert.equal(graphValueDelta(-100,group,[5,15],0,100),-10);
assert.deepEqual(graphBatch(group,[5,15],{kind:'shiftTime',delta:10}).map(k=>k.frame),[15,25,30]);
assert.deepEqual(graphBatch(group,[5,15],{kind:'shiftValue',delta:20}).map(k=>k.value),[30,50,80]);
assert.deepEqual(graphBatch(group,[5,15],{kind:'delete'}),[group[2]]);
for(let mask=1;mask<8;mask++){
 const frames=group.filter((_,i)=>mask&(1<<i)).map(k=>k.frame);
 for(let raw=-40;raw<130;raw++){
  const delta=graphTimeDelta(raw,frames,group,120),result=graphBatch(group,frames,{kind:'shiftTime',delta});
  assert.ok(result.every(k=>k.frame>=0&&k.frame<120));assert.equal(new Set(result.map(k=>k.frame)).size,3);
  const valid=Array.from({length:120},(_,i)=>i-Math.min(...frames)).filter(d=>frames.every(f=>f+d>=0&&f+d<120&&!group.some(k=>!frames.includes(k.frame)&&k.frame===f+d)));
  const target=Math.max(-Math.min(...frames),Math.min(119-Math.max(...frames),Math.round(raw)));
  valid.sort((a,b)=>Math.abs(a-target)-Math.abs(b-target)||a-b);assert.equal(delta,valid[0]);
 }
}
console.log('Multi-key batches: common delta, boundaries, collisions, values, interpolation and delete passed');

assert.deepEqual(graphBatch(group,[5,15],{kind:'interpolation',interpolation:'hold'}).map(k=>k.interpolation),['hold','hold','hold']);
assert.equal(graphBatch([{frame:1,value:65000,interpolation:'linear'}],[1],{kind:'shiftValue',delta:.01})[0].value,Math.fround(65000+Math.fround(.01)));

const {graphFullView,graphConstrainView,graphZoomView,graphPanView,graphFitView}=await import('../src/timeline-graph.ts');
const limits={min:-65536,max:65536},full=graphFullView(group,0,120);
for(const axis of ['both','time','value']){
 const zoomed=graphZoomView(full,2,axis,.3,.7,120,limits);
 if(axis!=='value'){assert.ok(Math.abs((zoomed.end-zoomed.start)-(full.end-full.start)/2)<1e-9);assert.ok(Math.abs(zoomed.start+.3*(zoomed.end-zoomed.start)-(.3*119))<1e-9);}
 else assert.equal(zoomed.start,full.start);
 if(axis!=='time')assert.ok(Math.abs(zoomed.min+.7*(zoomed.max-zoomed.min)-(full.min+.7*(full.max-full.min)))<1e-9);
 else assert.equal(zoomed.min,full.min);
 const value=(zoomed.min+zoomed.max)/2,p=graphPoint(15,value,120,zoomed,zoomed),v=graphValue(p.y,zoomed,-65536,65536);assert.ok(Math.abs(v-value)<=.0051);
 const at=zoomed.start+(p.x-GRAPH.left)/200*(zoomed.end-zoomed.start);assert.ok(Math.abs(at-15)<1e-9);
}
const z=graphZoomView(full,2,'both',.5,.5,120,limits),pan=graphPanView(z,5,10,120,limits);
assert.equal(pan.start,z.start+5);assert.equal(pan.min,z.min+10);
assert.equal(pan.end-pan.start,z.end-z.start);assert.equal(pan.max-pan.min,z.max-z.min);
for(const duration of [1,2,120,18000]){
 let view=graphFullView(group,0,duration);
 for(let i=0;i<50;i++)view=graphZoomView(view,10,'both',.1,.9,duration,limits);
 assert.ok(view.end>view.start&&view.max>view.min);assert.ok(view.end-view.start>=1-1e-9);assert.ok(view.max-view.min>=.01-1e-9);
 view=graphPanView(view,1e9,-1e9,duration,limits);assert.ok(view.start>=0&&view.end<=Math.max(1,duration-1));
 view=graphConstrainView(view,1,limits);assert.equal(view.start,0);assert.equal(view.end,1);
}
const fitted=graphFitView(group.slice(0,2),0,120,limits);
assert.ok(fitted.start<5&&fitted.end>15&&fitted.min<10&&fitted.max>30);
const singleFit=graphFitView(group.slice(0,1),0,120,limits);assert.ok(singleFit.end>singleFit.start&&singleFit.max>singleFit.min);
assert.deepEqual(graphFitView([],0,120,limits),graphFullView([],0,120));
assert.ok(graphPath(group,120,fitted,0,fitted).includes(' C '));
console.log('Graph viewport: independent axes, anchored zoom, inverse coordinates, pan spans, bounds, single-frame/empty/selection fit passed');

const {graphOwnsKey}=await import('../src/timeline-graph.ts');
for(const key of ['f','F','0','+','-','ArrowLeft','Delete'])assert.equal(graphOwnsKey(key,false,false),true);
assert.equal(graphOwnsKey('a',true,false),true);assert.equal(graphOwnsKey('ArrowRight',false,true),true);
for(const key of ['z','s','f'])assert.equal(graphOwnsKey(key,true,false),false);
assert.equal(graphOwnsKey('f',false,true),false);assert.equal(graphOwnsKey('b',false,false),false);
console.log('Graph shortcut scope: local navigation and selection reserved; global undo/save/tool keys retained');

const {graphDragEdit,graphEditTimeDelta}=await import('../src/timeline-graph.ts');
assert.deepEqual(graphDragEdit('free',10,5),{kind:'move',frameDelta:10,valueDelta:5});
assert.deepEqual(graphBatch(group,[5,15],graphDragEdit('free',10,5)).map(k=>[k.frame,k.value,k.interpolation]),[[15,15,'ease'],[25,35,'linear'],[30,80,'hold']]);
assert.equal(graphEditTimeDelta(graphDragEdit('free',10,5)),10);
assert.deepEqual(graphDragEdit('time',10,5),{kind:'shiftTime',delta:10});
assert.deepEqual(graphDragEdit('value',10,5),{kind:'shiftValue',delta:5});
assert.deepEqual(graphBatch(group,[5,15],graphDragEdit('free',0,5)).map(k=>k.frame),[5,15,30]);
console.log('Simultaneous graph edit: common time/value offsets, interpolation and single-axis mode isolation passed');

const {DEFAULT_KEY_CURVE,sampleKeyCurve,curveControl,equalKeyCurve}=await import('../src/key-curve.ts');
for(let i=0;i<=100;i++){
 const t=i/100;assert.ok(Math.abs(sampleKeyCurve(DEFAULT_KEY_CURVE,t)-t*t*(3-2*t))<.00001);
 assert.ok(Math.abs(sampleKeyCurve({x1:.1,y1:.1,x2:.8,y2:.8},t)-t)<.00001);
}
assert.equal(curveControl(DEFAULT_KEY_CURVE,1,1,-1).x1,DEFAULT_KEY_CURVE.x2);
assert.equal(curveControl(DEFAULT_KEY_CURVE,2,-1,2).x2,DEFAULT_KEY_CURVE.x1);
assert.equal(curveControl(DEFAULT_KEY_CURVE,1,.3,-1).y1,0);
assert.equal(curveControl(DEFAULT_KEY_CURVE,2,.7,2).y2,1);
const curve={x1:.1,y1:.8,x2:.4,y2:1},cubicKeys=[{frame:0,value:0,interpolation:'cubic',curve},{frame:100,value:100,interpolation:'linear'}];
for(const frame of [0,1,10,25,50,90,99,100])assert.ok(Math.abs(sampleTimelineKey(cubicKeys,frame,0)-sampleKeyCurve(curve,frame/100)*100)<.006);
assert.ok(graphPath(cubicKeys,101,{min:0,max:100},0).includes('C 52 36.8 112 12 232 12'));
const preview=graphBatch(group,[5],{kind:'curve',curve});assert.equal(preview[0].interpolation,'cubic');assert.ok(equalKeyCurve(preview[0].curve,curve));
assert.deepEqual(graphBatch(preview,[5],{kind:'move',frameDelta:2,valueDelta:1})[0].curve,preview[0].curve);
console.log('Custom tangents: easing inversion, exact curve geometry, constraints, f32 metadata and move retention passed');
