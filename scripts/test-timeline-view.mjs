import assert from 'node:assert/strict';
import {visibleFrames,visibleKeys,overlapsFrames,sampleTimelineKey,motionDragStart,snapTimelineFrame} from '../src/timeline-view.ts';
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

assert.equal(motionDragStart(10,20,2,24,8,16),12,'pointer and scroll displacement both affect the preview');
assert.equal(motionDragStart(10,20,2,-1000,0,16),0,'motion stays at or after the first frame');
assert.equal(motionDragStart(17950,20,2,1000,0,16),17960,'the repeated clip stays within the frame limit');
assert.equal(motionDragStart(10,20,2,7,0,16),10,'sub-frame motion does not commit a new frame');
console.log('Motion drag frame and scroll bounds passed');

assert.deepEqual(snapTimelineFrame(9.6,10,16,0,99,true,true),{frame:10,snapped:true});
assert.deepEqual(snapTimelineFrame(9.4,10,16,0,99,true,true),{frame:9,snapped:false});
assert.deepEqual(snapTimelineFrame(9.6,10,16,0,99,false,true),{frame:10,snapped:false});
assert.deepEqual(snapTimelineFrame(9.6,10,16,0,99,true,false),{frame:10,snapped:false});
assert.deepEqual(snapTimelineFrame(99.8,100,4,0,99,true,true),{frame:99,snapped:false});
assert.deepEqual(snapTimelineFrame(-3,0,16,0,99,true,true),{frame:0,snapped:false});
console.log('Playhead snap threshold, disabled state and bounds passed');

const {timelineLayerHeight,timelineLayerLayout,visibleTimelineLayers}=await import('../src/timeline-view.ts');
assert.equal(timelineLayerHeight(true,true,72,0,false),30);
assert.equal(timelineLayerHeight(true,false,56,0,false),82);
assert.equal(timelineLayerHeight(false,false,56,2,true),44+76+168);
const layers=Array.from({length:4096},(_,id)=>({id:String(id)}));
const layout=timelineLayerLayout(layers,()=>82);
assert.equal(layout.height,26+4096*82);
for(const top of [0,10000,layout.height-300]){
 const rendered=visibleTimelineLayers(layout.rows,top,300);
 const expected=layout.rows.flatMap((r,i)=>r.top+r.height>Math.max(0,top-160)&&r.top<top+460?[i]:[]);
 assert.deepEqual(rendered,expected);
 assert.ok(rendered.length<=9,'mounted layer blocks stay bounded');
}
assert.ok(visibleTimelineLayers(layout.rows,10000,300,['0','4095']).includes(0));
assert.ok(visibleTimelineLayers(layout.rows,10000,300,['0','4095']).includes(4095));
assert.deepEqual(visibleTimelineLayers([],0,300),[]);
const mixed=timelineLayerLayout(layers.slice(0,4),l=>[30,82,288,44][Number(l.id)]);
assert.deepEqual(mixed.rows.map(r=>r.top),[26,56,138,426]);
assert.deepEqual(visibleTimelineLayers(mixed.rows,138,288,[],0),[2]);
console.log('Vertical timeline mixed heights, exact edges, 4096-layer bounds and retained focus/capture passed');

const {timelineSnapTargets}=await import('../src/timeline-view.ts');
const snapTracks=[{id:'a',channels:{x:[{frame:10},{frame:30}],y:[{frame:18}]},exposures:[],motions:[{id:'moving',start:5,duration:10,repeats:2},{id:'fixed',start:60,duration:5,repeats:2}]},{id:'b',channels:{x:[{frame:30},{frame:40}]},exposures:[{start:12,end:24}],motions:[]}];
const keyTargets=timelineSnapTargets(snapTracks,30,120,{layer:'a',property:'x',frame:10});
assert.ok(!keyTargets.includes(10),'the moving key is not its own snap target');
assert.ok(!keyTargets.includes(30),'an occupied key cannot be overwritten by snapping, even at the playhead or another track key');
for(const frame of [0,12,18,24,40,60,70,120])assert.ok(keyTargets.includes(frame));
assert.deepEqual(snapTimelineFrame(17.6,keyTargets,16,0,119,true,true),{frame:18,snapped:true});
assert.deepEqual(snapTimelineFrame(23.4,keyTargets,16,0,119,true,true),{frame:23,snapped:false});
assert.equal(snapTimelineFrame(17.6,keyTargets,16,0,119,false,true).snapped,false);
assert.equal(snapTimelineFrame(17.6,keyTargets,16,0,119,true,false).snapped,false);
const clipTargets=timelineSnapTargets(snapTracks,90,120,{layer:'a',clip:'moving',span:20});
assert.ok(!clipTargets.includes(5)&&!clipTargets.includes(25),'the moving clip contributes neither edge');
assert.ok(clipTargets.includes(4),'the clip end can align to a cel end at frame 24');
assert.deepEqual(snapTimelineFrame(3.7,clipTargets,16,0,100,true,true),{frame:4,snapped:true});
assert.deepEqual(snapTimelineFrame(15,[10,20],1,0,100,true,true),{frame:10,snapped:true},'ties use the earlier target');
// Compare binary lookup with an independent bounded nearest-target reference.
for(const raw of [-10,-.4,0,3.4,18.6,29.7,70.2,100,120])for(const zoom of [4,16,32]){
 const candidates=keyTargets.filter(v=>v>=0&&v<=100).sort((a,b)=>Math.abs(a-raw)-Math.abs(b-raw)||a-b);
 const target=candidates[0],snapped=Math.abs(target-raw)*zoom<=8;
 assert.deepEqual(snapTimelineFrame(raw,keyTargets,zoom,0,100,true,true),{frame:Math.max(0,Math.min(100,Math.round(snapped?target:raw))),snapped});
}
console.log('Cross-track key/cel/clip targets, occupied-key protection, both clip edges, thresholds and nearest-target lookup passed');

const {exposureSnapPlan,snapExposureDelta}=await import('../src/timeline-view.ts');
const exposureTracks=[{id:'a',channels:{},motions:[],exposures:[{start:2,end:6},{start:12,end:16},{start:30,end:34}]},{id:'b',channels:{},motions:[],exposures:[{start:8,end:10},{start:35,end:38}]},{id:'keys',channels:{x:[{frame:24},{frame:40}]},motions:[],exposures:[]}];
const groupPlan=exposureSnapPlan(exposureTracks,0,120,{a:[2,12],b:[8]},{layer:'a',start:2,end:6},'move',false);
assert.deepEqual(groupPlan.edges,[2,6,8,10,12,16]);
assert.deepEqual(groupPlan.targets,[0,24,30,34,35,38,40,120]);
assert.deepEqual(snapExposureDelta(11.125,groupPlan,8,-2,14,true,true),{delta:12,snapped:true});
assert.deepEqual(snapExposureDelta(11.125,groupPlan,8,-2,14,false,true),{delta:11,snapped:false});
assert.deepEqual(snapExposureDelta(11.125,groupPlan,8,-2,14,true,false),{delta:11,snapped:false});
const shifted=exposureTracks.slice(0,2).flatMap(track=>track.exposures.filter(e=>({a:[2,12],b:[8]})[track.id].includes(e.start)).map(e=>dragExposure(e,12,'move',0,120)));
assert.deepEqual(shifted.map(e=>[e.start,e.end]),[[14,18],[24,28],[20,22]]);
const ripplePlan=exposureSnapPlan(exposureTracks,0,120,{a:[2],b:[8]},{layer:'a',start:2,end:6},'move',true);
assert.deepEqual(ripplePlan.targets,[0,24,40,120]);
assert.ok(!ripplePlan.edges.includes(38));assert.ok(ripplePlan.edges.includes(10));
const startPlan=exposureSnapPlan(exposureTracks,0,120,{a:[2,12],b:[8]},{layer:'a',start:2,end:6},'start',false);
assert.deepEqual(startPlan.edges,[2]);assert.ok(startPlan.targets.includes(12));assert.ok(!startPlan.targets.includes(6));
const endPlan=exposureSnapPlan(exposureTracks,0,120,{a:[2]},{layer:'a',start:2,end:6},'end',true);
assert.deepEqual(endPlan.edges,[6]);assert.ok(!endPlan.targets.includes(30));assert.ok(endPlan.targets.includes(8));
// Every snapped common delta must satisfy the intersection of all movement bounds.
for(const raw of [-20,-2.4,0,.5,4.1,11.125,13.8,25])for(const zoom of [4,8,16,32]){
 let best;let distance=Infinity;
 for(const edge of groupPlan.edges)for(const target of groupPlan.targets){const delta=target-edge,error=Math.abs(delta-raw);if(delta>=-2&&delta<=14&&error*zoom<=8&&(error<distance||error===distance&&(best===undefined||delta<best))){best=delta;distance=error;}}
 const expected=best===undefined?{delta:Math.max(-2,Math.min(14,Math.round(raw))),snapped:false}:{delta:best,snapped:true};
 assert.deepEqual(snapExposureDelta(raw,groupPlan,zoom,-2,14,true,true),expected);
}
assert.deepEqual(snapExposureDelta(1,{targets:[2],edges:[2]},8,1,10,true,true),{delta:1,snapped:false},'an end trim cannot collapse an exposure');
console.log('Exposure moves/trims, cross-track common deltas, moving-target exclusion, ripple followers and bounded nearest-pair scan passed');

const {defaultTimelineSnapFilters}=await import('../src/timeline-view.ts');
const filterTracks=[{id:'a',channels:{x:[{frame:5},{frame:30}]},exposures:[{start:8,end:12}],motions:[{id:'clip',start:16,duration:4,repeats:2}]}];
const categories={playhead:[3],keys:[5,30],exposures:[8,12],motions:[16,24],bounds:[0,120]};
for(let mask=0;mask<32;mask++){
 const filters=Object.fromEntries(Object.keys(categories).map((key,index)=>[key,Boolean(mask&(1<<index))]));
 const expected=[...new Set(Object.entries(categories).flatMap(([key,values])=>filters[key]?values:[]))].sort((a,b)=>a-b);
 const keyTargets=timelineSnapTargets(filterTracks,3,120,{layer:'a',property:'y',frame:30},filters);
 assert.deepEqual(keyTargets,expected,'each category contributes targets only when enabled');
 const exposurePlan=exposureSnapPlan(filterTracks,3,120,{a:[8]},{layer:'a',start:8,end:12},'move',false,filters);
 assert.deepEqual(exposurePlan.edges,[8,12],'filtering references does not remove the moving geometry');
 assert.ok(!exposurePlan.targets.includes(8)&&!exposurePlan.targets.includes(12),'moving exposures remain excluded');
 if(!filters.exposures)assert.ok(!keyTargets.includes(8));
}
assert.deepEqual(timelineSnapTargets(filterTracks,3,120,{layer:'a',property:'x',frame:5}),timelineSnapTargets(filterTracks,3,120,{layer:'a',property:'x',frame:5},defaultTimelineSnapFilters));
const noKeys={...defaultTimelineSnapFilters,keys:false};
assert.ok(!timelineSnapTargets(filterTracks,30,120,{layer:'a',property:'x',frame:5},noKeys).includes(30),'occupied keys stay blocked even when key targets are disabled');
assert.deepEqual(snapExposureDelta(3,{targets:[],edges:[8,12]},8,-8,100,true,true),{delta:3,snapped:false});
console.log('All 32 snap-filter combinations, default compatibility, moving geometry and occupied-key protection passed');

const {exposureGroupHoldBounds,exposureGroupHoldPreview}=await import('../src/timeline-view.ts');
const holds=[{id:'a',pixel:true,locked:false,exposures:[{start:2,end:6},{start:12,end:16},{start:30,end:34}]},{id:'b',pixel:true,locked:false,exposures:[{start:8,end:10},{start:35,end:38}]}];
const held={a:[2,12],b:[8]};
assert.deepEqual(exposureGroupHoldBounds(holds,held,120,false),{min:1,max:10});
assert.deepEqual(exposureGroupHoldBounds(holds,held,120,true),{min:1,max:8987});
for(const ripple of [false,true]){
 const bounds=exposureGroupHoldBounds(holds,held,120,ripple);
 for(const length of [1,2,4,6,10,bounds.max,bounds.max+1]){
  const preview=exposureGroupHoldPreview(holds,held,length,ripple);let valid=true;
  for(const track of holds){let offset=0,previous=0;
   for(const e of track.exposures){const selected=held[track.id].includes(e.start);const size=selected?length:e.end-e.start,start=e.start+offset,end=start+size;
    assert.deepEqual(preview.get(track.id).get(e.start),{...e,start,end});
    if(start<previous||end>(ripple?18000:120)||end<=start)valid=false;
    previous=end;if(ripple&&selected)offset+=length-(e.end-e.start);
   }
  }
  assert.equal(valid,length<=bounds.max);
 }
}
assert.deepEqual([...exposureGroupHoldPreview(holds,held,6,true).get('a').values()].map(e=>[e.start,e.end]),[[2,8],[14,20],[34,38]]);
assert.deepEqual([...exposureGroupHoldPreview(holds,held,6,true).get('b').values()].map(e=>[e.start,e.end]),[[8,14],[39,42]]);
assert.deepEqual(exposureGroupHoldBounds([],{},120,false),{min:1,max:1});
console.log('Shared hold bounds, mixed lengths, cross-track previews, cumulative ripple offsets and 18000-frame limit passed');

const {visibleTimelineMotionRows,TIMELINE_MOTION_ROW_HEIGHT}=await import('../src/timeline-view.ts');
assert.equal(TIMELINE_MOTION_ROW_HEIGHT,38);
for(const count of [0,1,64,4096])for(const top of [-800,-76,0,38,777,155610])for(const height of [0,1,200,600]){
 const keep=[0,count-1,-1,count,1.5];
 const actual=visibleTimelineMotionRows(count,top,height,keep);
 const expected=Array.from({length:count},(_,i)=>i).filter(i=>i*38<top+height+76&&(i+1)*38>Math.max(0,top-76)||keep.includes(i));
 assert.deepEqual(actual,expected);
 assert.ok(actual.length<=Math.ceil(height/38)+7);
}
assert.deepEqual(visibleTimelineMotionRows(64,38,38,[],0),[1]);
assert.deepEqual(visibleTimelineMotionRows(64,38*64,100,[],0),[]);
console.log('Motion row virtualization: empty/64/4096 rows, viewport edges, bounded DOM, focus/capture retention passed');
