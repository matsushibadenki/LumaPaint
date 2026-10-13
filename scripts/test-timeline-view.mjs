import assert from 'node:assert/strict';
import {visibleFrames,visibleKeys,overlapsFrames,sampleTimelineKey} from '../src/timeline-view.ts';
const keys=Array.from({length:4096},(_,frame)=>({frame,value:frame,interpolation:'linear'}));
for(const scroll of [0,16000,65000,200000]) {
  const range=visibleFrames(scroll,800,16,18000);
  assert.ok(range.from>=0&&range.to<=18000&&range.to>range.from);
  assert.ok(range.to-range.from<=65);
  assert.deepEqual(visibleKeys(keys,range),keys.filter(k=>k.frame>=range.from&&k.frame<range.to));
}
const range=visibleFrames(16000,800,16,18000);
assert.ok(visibleKeys(keys,range,keys[0]).includes(keys[0]),'captured key stays mounted across scrolling');
assert.ok(overlapsFrames(0,18000,range),'a held cel whose start is off screen remains visible');
assert.equal(overlapsFrames(0,10,range),false);
assert.equal(overlapsFrames(range.to,range.to+1,range),false);
for(const interpolation of ['linear','hold','ease']) {
  const k=[{frame:0,value:0,interpolation},{frame:20,value:100,interpolation:'linear'}];
  assert.equal(sampleTimelineKey(k,5,0),interpolation==='linear'?25:interpolation==='hold'?0:15.63);
  assert.equal(sampleTimelineKey(k,20,0),100);
  assert.equal(sampleTimelineKey(k,40,0),100);
}
assert.equal(sampleTimelineKey([],5,100),100);
assert.equal(sampleTimelineKey([{frame:10,value:7,interpolation:'linear'}],0,0),7);
console.log('Timeline viewport, capture retention, interval boundaries and binary samples passed');

assert.equal(sampleTimelineKey([{frame:0,value:17.1234,interpolation:"linear"},{frame:10,value:20,interpolation:"linear"}],0,0),17.1234,"Authored key precision is retained in the inspector");

const {dragExposure}=await import('../src/timeline-view.ts');
const exposure={start:5,end:10,cel:2};
assert.deepEqual(dragExposure(exposure,20,'move',3,15),{start:10,end:15,cel:2});
assert.deepEqual(dragExposure(exposure,-20,'move',3,15),{start:3,end:8,cel:2});
assert.deepEqual(dragExposure(exposure,20,'start',3,15),{start:9,end:10,cel:2});
assert.deepEqual(dragExposure(exposure,-20,'end',3,15),{start:5,end:6,cel:2});
assert.deepEqual(dragExposure(exposure,2,'end',3,15),{start:5,end:12,cel:2});
const {exposureMoveBounds,selectExposureFrames}=await import('../src/timeline-view.ts');
const spans=[{start:0,end:2,cel:1},{start:3,end:5,cel:2},{start:6,end:8,cel:3},{start:9,end:11,cel:4}];
assert.deepEqual(exposureMoveBounds(spans,[0,6],12,false),{min:0,max:1});
assert.deepEqual(exposureMoveBounds(spans,[3,6],12,true),{min:-1,max:17989});
assert.deepEqual(selectExposureFrames(spans,[0],6,0,true,false),[0,3,6]);
assert.deepEqual(selectExposureFrames(spans,[0,6],6,0,false,true),[0]);
