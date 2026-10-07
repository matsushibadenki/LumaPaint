import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {stripTypeScriptTypes} from 'node:module';
async function load(path) { const code=stripTypeScriptTypes(readFileSync(new URL(path,import.meta.url),'utf8'));return import('data:text/javascript;base64,'+Buffer.from(code).toString('base64')); }
const {pixelsPerMeasurement}=await load('../src/measurement-math.ts');
const {rulerTicks}=await load('../src/ruler-math.ts');
for(const dpi of [.5,72,144,150.25,300.5,1200]) {
 assert.equal(pixelsPerMeasurement('pixels',dpi),1);
 assert.ok(Math.abs(pixelsPerMeasurement('millimeters',dpi)*25.4-dpi)<1e-9);
 assert.ok(Math.abs(pixelsPerMeasurement('centimeters',dpi)*2.54-dpi)<1e-9);
 assert.equal(pixelsPerMeasurement('points',dpi)*72,dpi);
 for(const zoom of [.0313,.5,1,640]) for(const origin of [-99999,-320,0,24,500]) {
  const factor=pixelsPerMeasurement('millimeters',dpi)*zoom,ticks=rulerTicks(1000,origin,factor);
  assert.ok(ticks.length<100);
  assert.ok(ticks.every(t=>t.position>=-1e-7 && t.position<=1000+1e-7));
  const majors=ticks.filter(t=>t.major);
  assert.ok(majors.length>0);
  for(let i=1;i<majors.length;i++)assert.ok(majors[i].position-majors[i-1].position>=79.99);
  for(const t of majors)assert.ok(Math.abs(origin+Number(t.label)*factor-t.position)<.001);
 }
}
assert.deepEqual(rulerTicks(0,0,1),[]);
assert.deepEqual(rulerTicks(100,0,0),[]);
console.log('Measurement conversion and ruler pan/zoom regression checks passed.');

// Non-square pixel density: 601 px equals 2 inches horizontally and 4 vertically.
assert.equal(601 / pixelsPerMeasurement('inches',300.5),2);
assert.equal(601 / pixelsPerMeasurement('inches',150.25),4);
assert.equal(pixelsPerMeasurement('inches',NaN),72);
assert.equal(pixelsPerMeasurement('inches',Infinity),72);
assert.equal(pixelsPerMeasurement('inches',0),72);
