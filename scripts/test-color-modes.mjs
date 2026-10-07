import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {stripTypeScriptTypes} from 'node:module';
const code=stripTypeScriptTypes(readFileSync('src/color-models.ts','utf8'));
const {rgbToLab,labToRgb,rgbToGray}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
for(const rgb of [[0,0,0],[255,255,255],[255,0,0],[0,255,0],[0,0,255],[128,128,128]]) {
 const lab=rgbToLab(rgb);assert.ok(lab.every(Number.isFinite));const actual=labToRgb(lab);assert.deepEqual(actual,rgb);
}
const white=rgbToLab([255,255,255]);assert.ok(Math.abs(white[0]-100)<.01&&Math.abs(white[1])<.01&&Math.abs(white[2])<.01);
const red=rgbToLab([255,0,0]);assert.ok(Math.abs(red[0]-54.29)<.05&&Math.abs(red[1]-80.8)<.05&&Math.abs(red[2]-69.89)<.05);
for(let r=0;r<256;r+=17)for(let g=0;g<256;g+=17)for(let b=0;b<256;b+=17) assert.deepEqual(labToRgb(rgbToLab([r,g,b])),[r,g,b]);
assert.equal(rgbToGray([255,0,0]),127);assert.equal(rgbToGray([0,255,0]),220);assert.equal(rgbToGray([0,0,255]),76);
assert.ok(labToRgb([50,-128,127]).every(v=>Number.isInteger(v)&&v>=0&&v<=255));
console.log('Lab D50 round trip, reference colors, gray luminance and gamut clipping passed.');
