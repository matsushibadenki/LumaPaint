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

const {curveSlope,velocityData,velocityAt}=await import('../src/timeline-velocity.ts');
const vk=(interpolation,curve)=>[{frame:0,value:0,interpolation,curve},{frame:20,value:100,interpolation:'linear'}];
assert.equal(velocityAt(vk('linear'),24,5),120);
assert.equal(velocityAt(vk('ease'),24,10),180);
assert.equal(velocityAt(vk('ease'),24,5),135);
assert.equal(velocityAt(vk('hold'),24,10),0);
assert.equal(velocityAt(vk('hold'),24,20),null);
assert.deepEqual(velocityData(vk('hold'),24,40).jumps,[20]);
assert.equal(velocityAt(vk('linear'),24,0),null); // a derivative discontinuity, not a fabricated endpoint rate
assert.equal(velocityAt(vk('linear'),24,21),0);
assert.equal(velocityAt([{frame:10,value:5,interpolation:'ease'}],24,10),0);
for(const c of [DEFAULT_KEY_CURVE,{x1:.1,y1:.8,x2:.4,y2:1},{x1:.2,y1:.2,x2:.8,y2:.8}]){
 for(const time of [.1,.25,.5,.75,.9]){
  const epsilon=.0001,expected=(sampleKeyCurve(c,time+epsilon)-sampleKeyCurve(c,time-epsilon))/(2*epsilon)*120;
  assert.ok(Math.abs(velocityAt(vk('cubic',c),24,time*20)-expected)<.15);
 }
}
assert.equal(curveSlope({x1:0,y1:1,x2:.5,y2:1},0),null);
assert.equal(curveSlope({x1:0,y1:0,x2:.5,y2:.7},0),1.4);
assert.equal(curveSlope({x1:0,y1:0,x2:0,y2:0},0),1);
assert.deepEqual(velocityData(vk('cubic',{x1:0,y1:1,x2:.5,y2:1}),24,40).singular,[0]);
const negative=vk('ease').map(k=>({...k,value:100-k.value}));assert.equal(velocityAt(negative,24,10),-180);
assert.equal(velocityAt(vk('linear'),48,5),240);
for(const data of [velocityData([],24,1),velocityData(negative,24,40),velocityData(vk('cubic',{x1:0,y1:1,x2:.5,y2:1}),24,40)]){
 assert.ok(data.min<0&&data.max>0);assert.ok(data.lines.flat().every(p=>Number.isFinite(p.frame)&&Number.isFinite(p.value)));
}
const denseVelocity=velocityData(Array.from({length:4096},(_,frame)=>({frame,value:frame%2,interpolation:'cubic'})),24,4096);assert.ok(denseVelocity.lines.flat().length<=13000);
const zoomedVelocity=velocityData(vk('ease'),24,40,9,11);assert.ok(zoomedVelocity.lines.flat().every(p=>p.frame>=9&&p.frame<=11));
console.log('Velocity graph: signed units/second, fps, easing derivatives, cubic inversion, stationary/vertical tangents, jumps, boundaries, zoom and bounded samples passed');

// Linked tangents preserve physical slope even with unequal time/value intervals.
const linkKeys=[{frame:0,value:0,interpolation:'cubic'},{frame:10,value:20,interpolation:'cubic'},{frame:40,value:80,interpolation:'linear'}];
const linkSlope=(keys,i,c)=>{const a=keys[i],b=keys[i+1],curve=a.curve;return (b.value-a.value)/(b.frame-a.frame)*(c===1?curve.y1/curve.x1:(1-curve.y2)/(1-curve.x2));};
for(const [frame,control] of [[0,2],[10,1]]){
 const curve={x1:.25,y1:.4,x2:.75,y2:.6},edited=graphBatch(linkKeys,[frame],{kind:'linkedCurve',curve,control});
 assert.ok(Math.abs(linkSlope(edited,0,2)-linkSlope(edited,1,1))<1e-5);
 assert.deepEqual(edited.map(k=>[k.frame,k.value]),[[0,0],[10,20],[40,80]]);
 assert.deepEqual(graphBatch(edited,[frame],{kind:'linkedCurve',curve,control}),edited);
}
for(const end of [-20,20,80]){
 const keys=linkKeys.map((k,i)=>i===2?{...k,value:end}:k),edited=graphBatch(keys,[0],{kind:'linkedCurve',control:2,curve:{x1:0,y1:1,x2:1,y2:0}});
 assert.ok(Math.abs(linkSlope(edited,0,2)-linkSlope(edited,1,1))<.001*Math.max(1,Math.abs(linkSlope(edited,0,2))));
 if(end<=20)assert.equal(linkSlope(edited,0,2),0);
 for(const k of edited.filter(k=>k.curve)){assert.ok(k.curve.x1<=k.curve.x2);assert.ok(Object.values(k.curve).every(n=>n>=0&&n<=1));}
}
const unlinked=linkKeys.map((k,i)=>i===1?{...k,interpolation:'hold'}:k);
assert.deepEqual(graphBatch(unlinked,[0],{kind:'linkedCurve',control:2,curve:DEFAULT_KEY_CURVE})[1],unlinked[1]);
console.log('Linked tangents: unequal interval slopes, both directions, extrema, flat spans, vertical handles, bounds, idempotence and independent hold neighbors passed');

const {allPropertyKeys,selectPropertyKeys,propertyKeyGroups,keyGroupDelta,propertyKeysOwnKey}=await import('../src/timeline-key-group.ts');
const channels={x:[{frame:5},{frame:15},{frame:30}],y:[{frame:5},{frame:18}],rotation:[{frame:10},{frame:40}]};
const anchor={property:'x',frame:5},other={property:'rotation',frame:10};
let picked=selectPropertyKeys(channels,[],anchor,null,false,false);
picked=selectPropertyKeys(channels,picked,other,anchor,true,false);assert.equal(picked.length,2);
assert.deepEqual(selectPropertyKeys(channels,picked,other,anchor,true,false),[anchor]);
assert.deepEqual(selectPropertyKeys(channels,[],other,anchor,false,true),[{property:'x',frame:5},{property:'y',frame:5},{property:'rotation',frame:10}]);
assert.equal(allPropertyKeys(channels).length,7);assert.equal(propertyKeyGroups(picked).length,2);
assert.equal(keyGroupDelta(5,channels,picked,48),5);
assert.equal(keyGroupDelta(10,channels,picked,48,1),11); // x's unselected frame15 blocks10 for the whole group.
assert.equal(keyGroupDelta(-99,channels,picked,48),-5);assert.equal(keyGroupDelta(100,channels,picked,48),37);
assert.equal(keyGroupDelta(1,{x:[{frame:0},{frame:1}]},[{property:'x',frame:0}],2,1),0);
assert.equal(selectPropertyKeys({x:Array.from({length:5000},(_,frame)=>({frame}))},[],{property:'x',frame:4999},{property:'x',frame:0},false,true).length,4096);
for(const key of ['ArrowLeft','ArrowRight','Delete','Escape'])assert.equal(propertyKeysOwnKey(key,false,false),true);
for(const key of ['z','s','f'])assert.equal(propertyKeysOwnKey(key,true,false),false);
assert.equal(propertyKeysOwnKey('a',true,false),true);assert.equal(propertyKeysOwnKey('ArrowLeft',false,true),false);
console.log('Cross-property keys: toggle/range/all selection, common collision-free time offset, bounds, capacity and shortcut ownership passed');

const {propertyKeyDragPlan,propertyKeyDragDelta,propertyKeySnapTargets,visiblePropertyKeys}=await import('../src/timeline-key-group.ts');
const groupChannels={x:[{frame:5,value:0},{frame:25,value:1}],y:[{frame:10,value:0},{frame:30,value:1}]},groupPicked=[{property:'x',frame:5},{property:'y',frame:10}];
let groupPlan=propertyKeyDragPlan(groupChannels,groupPicked,48,[20]);
assert.deepEqual(propertyKeyDragDelta(9.7,groupPlan,16,true,true),{delta:10,snapped:true}); // the non-driver edge at10 reaches20
assert.deepEqual(propertyKeyDragDelta(9.7,groupPlan,16,false,true),{delta:10,snapped:false});
assert.deepEqual(propertyKeyDragDelta(.3,groupPlan,16,true,false),{delta:0,snapped:false});
assert.deepEqual(propertyKeyDragDelta(-99,groupPlan,16,true,true),{delta:-5,snapped:false});
assert.deepEqual(propertyKeyDragDelta(99,groupPlan,16,true,true),{delta:37,snapped:false});
groupPlan=propertyKeyDragPlan({x:[{frame:5},{frame:15}],y:[{frame:10}]},groupPicked,48,[20,21]);
assert.deepEqual(propertyKeyDragDelta(9.7,groupPlan,4,true,true),{delta:11,snapped:true}); // nearest snap collides; next valid candidate wins
assert.deepEqual(propertyKeyDragDelta(10,groupPlan,16,false,true),{delta:9,snapped:false});
const snapTracks=[{id:'a',channels:groupChannels,exposures:[],motions:[]},{id:'b',channels:{rotation:[{frame:16,value:0}]},exposures:[{start:12,end:24}],motions:[{id:'clip',start:40,duration:3,repeats:2}]}];
const filters={playhead:false,keys:true,exposures:false,motions:false,bounds:false};
assert.deepEqual(propertyKeySnapTargets(snapTracks,'a',groupPicked,7,48,filters),[16,25,30]);
for(let mask=0;mask<32;mask++){
 const names=['playhead','keys','exposures','motions','bounds'],f=Object.fromEntries(names.map((n,i)=>[n,!!(mask&(1<<i))])),targets=new Set(propertyKeySnapTargets(snapTracks,'a',groupPicked,7,48,f));
 for(const [name,values] of Object.entries({playhead:[7],keys:[16,25,30],exposures:[12,24],motions:[40,46],bounds:[0,47,48]}))for(const v of values)assert.equal(targets.has(v),f[name]);
 assert.ok(!targets.has(5)&&!targets.has(10));
}
assert.deepEqual(visiblePropertyKeys([{frame:1},{frame:8},{frame:90}],{from:0,to:10},new Set([90]),-85,90).map(k=>k.frame),[1,8,90]);
assert.deepEqual(visiblePropertyKeys([{frame:1},{frame:8},{frame:90}],{from:0,to:10},new Set([1]),20,1).map(k=>k.frame),[1,8]);
const many=Array.from({length:4096},(_,frame)=>({frame})),manyPicked=many.map(k=>({property:'x',frame:k.frame})),manyPlan=propertyKeyDragPlan({x:many},manyPicked,18000,[12000]);
assert.deepEqual(propertyKeyDragDelta(7905.1,manyPlan,4,true,true),{delta:7905,snapped:true});
assert.ok(manyPlan.free.size<=18000);
console.log('Property group drag: non-driver snap, collision-safe candidates, bounds, threshold, bypass, all target filters, moving-key exclusion, virtual preview/capture and 4096-key plan passed');
const radiusPlan=propertyKeyDragPlan({x:[{frame:0}]},[{property:'x',frame:0}],48,[10]);
assert.deepEqual(propertyKeyDragDelta(8,radiusPlan,4,true,true),{delta:10,snapped:true});
assert.deepEqual(propertyKeyDragDelta(7.99,radiusPlan,4,true,true),{delta:8,snapped:false});
const tiePlan=propertyKeyDragPlan({x:[{frame:0}]},[{property:'x',frame:0}],48,[8,10]);
assert.deepEqual(propertyKeyDragDelta(9,tiePlan,4,true,true),{delta:8,snapped:true});

const {allLayerKeys,selectLayerKeys,layerKeyGroups,layerKeyDragPlan,layerKeyDelta,layerKeySnapTargets}=await import('../src/timeline-key-tracks.ts');
const crossTracks=[{id:'one',pixel:false,locked:false,channels:{x:[{frame:5},{frame:15}]}},{id:'locked',pixel:false,locked:true,channels:{x:[{frame:5}]}},{id:'two',pixel:false,locked:false,channels:{x:[{frame:5},{frame:20}]}}];
const crossPicked=[{layer:'one',property:'x',frame:5},{layer:'two',property:'x',frame:5}];
assert.equal(allLayerKeys(crossTracks).length,4);assert.equal(layerKeyGroups(crossPicked).length,2);
assert.deepEqual(selectLayerKeys(crossTracks,[crossPicked[0]],crossPicked[1],crossPicked[0],true,false),crossPicked);
assert.deepEqual(selectLayerKeys(crossTracks,[],crossPicked[1],crossPicked[0],false,true),crossPicked);
const crossPlan=layerKeyDragPlan(crossTracks,crossPicked,48,[16]);assert.equal(layerKeyDelta(15,crossPlan,1),16); // second layer's frame20 blocks15 for all layers
assert.deepEqual(propertyKeyDragDelta(10.8,crossPlan,16,true,true),{delta:11,snapped:true});
const crossTargets=layerKeySnapTargets(crossTracks.map(l=>({...l,exposures:[],motions:[]})),crossPicked,7,48,{playhead:false,keys:true,exposures:false,motions:false,bounds:false});assert.deepEqual(crossTargets,[5,15,20]); // unselected locked-layer key remains a valid target
assert.equal(allLayerKeys([{id:'many',pixel:false,locked:false,channels:{x:Array.from({length:5000},(_,frame)=>({frame}))}}]).length,4096);
console.log('Cross-layer keys: distinct identities, toggle/range/all, locked exclusion, atomic group plans, common collision offset, snapping and bounded selection passed');

const {selectedValueBounds,valueOffsetsValid,keyValueUnit}=await import('../src/timeline-key-values.ts');
const valueTracks=[{id:'a',pixel:false,locked:false,channels:{x:[{frame:0,value:10}],y:[{frame:0,value:65530}],rotation:[{frame:0,value:90}],scaleX:[{frame:0,value:100}],opacity:[{frame:0,value:40}]}},{id:'b',pixel:false,locked:false,channels:{opacity:[{frame:5,value:90}],scaleY:[{frame:5,value:.5}]}}];
const valueSelection=allLayerKeys(valueTracks),unitBounds=selectedValueBounds(valueTracks,valueSelection),zeroOffsets={position:0,rotation:0,scale:0,opacity:0};
assert.deepEqual([unitBounds.position.min,unitBounds.position.max,unitBounds.position.count],[-65546,6,2]);assert.deepEqual([unitBounds.opacity.min,unitBounds.opacity.max,unitBounds.opacity.count],[-40,10,2]);assert.equal(unitBounds.scale.count,2);assert.equal(unitBounds.scale.min,-.4);
assert.equal(keyValueUnit('x'),'position');assert.equal(keyValueUnit('scaleY'),'scale');assert.equal(keyValueUnit('opacity'),'opacity');
assert.ok(valueOffsetsValid(unitBounds,{position:5,rotation:-15,scale:10,opacity:-5}));
for(const offsets of [{...zeroOffsets,position:7},{...zeroOffsets,scale:-.5},{...zeroOffsets,opacity:11},{...zeroOffsets,rotation:NaN}])assert.equal(valueOffsetsValid(unitBounds,offsets),false);
assert.equal(selectedValueBounds(valueTracks,[{layer:'a',property:'x',frame:0}]).opacity.count,0);
console.log('Unit value batches: px/degree/scale/opacity separation, cross-layer intersection limits, bounds and nonfinite rejection passed');

const smallScaleBounds=selectedValueBounds([{id:'f32',pixel:false,locked:false,channels:{scaleX:[{frame:0,value:50}]}}],[{layer:'f32',property:'scaleX',frame:0}]);
assert.equal(valueOffsetsValid(smallScaleBounds,{...zeroOffsets,scale:-49.9}),false); // f32 addition would fall below0.1

const {valueOffsetsChanged}=await import('../src/timeline-key-values.ts');
assert.equal(valueOffsetsChanged(unitBounds,{...zeroOffsets,rotation:1e-8}),false);
assert.equal(valueOffsetsChanged(unitBounds,{...zeroOffsets,rotation:1}),true);

const {velocityControl,velocityCurve}=await import('../src/timeline-velocity.ts');
for(const fps of [12,24,60])for(const sign of [-1,1])for(const interpolation of ['linear','ease','cubic'])for(const control of [1,2]){
 const keys=[{frame:5,value:sign>0?10:90,interpolation,curve:{x1:Math.fround(.2),y1:Math.fround(.4),x2:Math.fround(.7),y2:Math.fround(.8)}},{frame:25,value:sign>0?90:10,interpolation:'linear'}];
 const info=velocityControl(...keys,fps,control);assert.ok(info);const value=sign*24,c=velocityCurve(...keys,fps,control,value);assert.ok(c);
 const next=graphBatch(keys,[5],{kind:'velocity',control,value},fps),updated=velocityControl(...next,fps,control);
 assert.ok(Math.abs(updated.value-value)<.0001);assert.deepEqual(next.map(k=>[k.frame,k.value]),keys.map(k=>[k.frame,k.value]));
 assert.deepEqual([c.x1,c.x2],[info.curve.x1,info.curve.x2]);assert.equal(control===1?c.y2:c.y1,control===1?info.curve.y2:info.curve.y1);
 assert.equal(velocityCurve(...keys,fps,control,-sign),null);assert.equal(velocityCurve(...keys,fps,control,sign*1e6),null);assert.equal(velocityCurve(...keys,fps,control,NaN),null);
}
const va={frame:0,value:0,interpolation:'cubic',curve:{x1:0,y1:.4,x2:1,y2:.6}},vb={frame:20,value:100,interpolation:'linear'};
for(const control of [1,2]){assert.equal(velocityControl(va,vb,24,control),null);assert.equal(velocityControl({...va,interpolation:'hold'},vb,24,control),null);assert.equal(velocityControl({...va,interpolation:'linear'},{...vb,value:0},24,control),null);}
console.log('Velocity editing: signed units, fps, both endpoints, linear/ease conversion, fixed time/opposite handles, bounded output and hold/flat/vertical guards passed');
