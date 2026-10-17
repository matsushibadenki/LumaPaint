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

const {timelineEdgeScrollSpeed}=await import('../src/timeline-view.ts');
assert.equal(timelineEdgeScrollSpeed(200,0,400),0);
assert.equal(timelineEdgeScrollSpeed(0,0,400),-720);
assert.equal(timelineEdgeScrollSpeed(380,0,400),360);
assert.equal(timelineEdgeScrollSpeed(900,0,400),720);
assert.equal(timelineEdgeScrollSpeed(-100,0,400),-720);
assert.equal(timelineEdgeScrollSpeed(0,10,10),0);
assert.equal(timelineEdgeScrollSpeed(NaN,0,400),0);
assert.ok(timelineEdgeScrollSpeed(5,0,30)<0);
assert.deepEqual(dragExposure(exposure,(20+48)/16,'move',0,30),{start:9,end:14,cel:2},'scroll displacement participates in drag timing');

const {selectedExposurePositions}=await import('../src/timeline-view.ts');
const identities=[{id:'a',start:6,end:9,cel:1},{id:'b',start:9,end:12,cel:1}];
assert.deepEqual(selectedExposurePositions(identities,['a','b'],'b'),{frames:[6,9],anchor:9});
assert.deepEqual(selectedExposurePositions([],['a','b'],'b'),{frames:[],anchor:null});
assert.deepEqual(selectedExposurePositions([{id:'a',start:0,end:3,cel:1},{id:'b',start:3,end:6,cel:1}],['a','b'],'b'),{frames:[0,3],anchor:3},'undo restores matching occurrences and anchor');
assert.deepEqual(selectedExposurePositions([{id:'new',start:6,end:9,cel:1}],['a'],'a'),{frames:[],anchor:null},'same cel and frame cannot steal old selection');

const {exposureDragGesture}=await import('../src/timeline-view.ts');
assert.equal(exposureDragGesture(0,0),false);
assert.equal(exposureDragGesture(2,0),false);
assert.equal(exposureDragGesture(0,40),true,'bounded move with edge scrolling must suppress click selection');
assert.equal(exposureDragGesture(-10,0),true,'bounded pointer move remains a drag');

assert.equal(exposureDragGesture(2,0,true),true,'a one-frame edit at narrow zoom still suppresses click');

const {exposureReorderTarget}=await import('../src/timeline-view.ts');
assert.deepEqual(exposureReorderTarget(spans,[0,6],3.9),{before:3,frame:3},'selected exposures are excluded from insertion targets');
assert.deepEqual(exposureReorderTarget(spans,[0,6],4),{before:9,frame:9},'crossing the target midpoint chooses the following slot');
assert.deepEqual(exposureReorderTarget(spans,[3],-20),{before:0,frame:0},'left edge inserts before the first remaining exposure');
assert.deepEqual(exposureReorderTarget(spans,[3],100),{before:null,frame:11},'right edge appends after the last exposure');
assert.equal(exposureReorderTarget(spans,[0,3,6,9],5),null,'there is no insertion target when all exposures are selected');
assert.equal(exposureReorderTarget(spans,[],5),null);
assert.equal(exposureReorderTarget(spans,[3],NaN),null);

console.log('Exposure insertion boundary checks passed');

const {selectExposureTracks,exposureGroupMoveBounds}=await import('../src/timeline-view.ts');
const crossTracks=[
 {id:'top',pixel:true,locked:false,exposures:[{id:'t1',start:0,end:2,cel:1},{id:'t2',start:4,end:6,cel:2},{id:'t3',start:8,end:10,cel:3}]},
 {id:'vector',pixel:false,locked:false,exposures:[]},
 {id:'locked',pixel:true,locked:true,exposures:[{start:0,end:12,cel:1}]},
 {id:'bottom',pixel:true,locked:false,exposures:[{id:'b1',start:1,end:3,cel:1},{id:'b2',start:5,end:7,cel:2},{id:'b3',start:9,end:11,cel:3}]},
];
let cross=selectExposureTracks(crossTracks,{tracks:{},anchor:null},'top',0,false,false);
cross=selectExposureTracks(crossTracks,cross,'bottom',1,false,true);
assert.deepEqual(cross.tracks,{top:[0],bottom:[1]},'modifier selection preserves occurrences in other tracks');
assert.deepEqual(exposureGroupMoveBounds(crossTracks,cross.tracks,12,false),{min:0,max:2},'all selected tracks share the strictest collision bounds');
assert.deepEqual(exposureGroupMoveBounds(crossTracks,cross.tracks,12,true),{min:0,max:17989},'ripple bounds include subsequent exposures in every participating track');
const rectangle=selectExposureTracks(crossTracks,cross,'top',4,true,false);
assert.deepEqual(rectangle.tracks,{top:[0,4],bottom:[1]},'range crosses tracks and includes held intervals but skips locked/vector tracks');
assert.deepEqual(selectExposureTracks(crossTracks,cross,'bottom',1,false,true).tracks,{top:[0]},'toggling the last occurrence removes only its own track');
assert.deepEqual(selectExposureTracks(crossTracks,cross,'top',4,false,false).tracks,{top:[4]},'plain click replaces the complete group');
assert.equal(selectExposureTracks(crossTracks,cross,'locked',0,false,false),cross);
assert.equal(selectExposureTracks(crossTracks,cross,'top',3,false,false),cross);
assert.deepEqual(exposureGroupMoveBounds(crossTracks,{locked:[0],top:[0]},12,false),{min:0,max:0});
const tight=crossTracks.map(t=>t.id==='bottom'?{...t,exposures:[{start:1,end:4,cel:1},{start:5,end:7,cel:2}]}:t);
assert.deepEqual(exposureGroupMoveBounds(tight,cross.tracks,12,false),{min:0,max:1},'a tighter second track constrains the source-track drag');
console.log('Cross-track selection, held-range and shared-drag bounds passed');
const unusual=[{...crossTracks[0],id:'__proto__'},{...crossTracks[3],id:'toString'}];
const unusualSelected=selectExposureTracks(unusual,{tracks:{},anchor:null},'__proto__',0,false,true);
assert.deepEqual(Object.keys(unusualSelected.tracks),['__proto__'],'opaque track IDs remain ordinary selection keys');
assert.deepEqual(exposureGroupMoveBounds(unusual,unusualSelected.tracks,12,false),{min:0,max:2},'unselected prototype-named tracks are not treated as selected');
