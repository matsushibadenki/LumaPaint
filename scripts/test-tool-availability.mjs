import assert from 'node:assert/strict';
import { toolUnavailableReason, toolbarTool } from '../src/tool-availability.ts';
const layer = { id: 'one', kind: 'paint', visible: true, locked: false };
const context = (kind = 'paint', extra = {}, doc = {}) => ({ available: true, editable: true, busy: false,
  document: { layers: [{ ...layer, kind, ...extra }], layerId: 'one', layerEditTarget: 'content', layerGroups: { members: [], groups: [] }, ...doc } });
const allowed = (tool, ctx) => !toolUnavailableReason(tool, ctx, 'en');
for (const tool of ['brush', 'eraser', 'blur', 'sharpen', 'smudge', 'cloneStamp', 'paintBucket']) {
  assert.ok(allowed(tool, context()));
  assert.ok(!allowed(tool, context('vector')));
  assert.ok(!allowed(tool, context('svg')));
  assert.ok(!allowed(tool, context('paint', { locked: true })));
}
for (const tool of ['vectorPen', 'vectorDirectSelect', 'vectorScale', 'vectorRectangle', 'text', 'textVertical', 'textFrame', 'imageFrameRectangle']) {
  assert.ok(allowed(tool, context('vector')));
  assert.ok(!allowed(tool, context()));
  assert.ok(!allowed(tool, context('svg')));
  assert.ok(!allowed(tool, context('vector', {}, { layerEditTarget: 'mask' })));
}
for (const tool of ['hand', 'zoomIn', 'zoomOut', 'eyedropper']) {
  assert.ok(allowed(tool, { ...context('paint', { locked: true }), editable: false, busy: true }));
  assert.ok(!allowed(tool, { ...context(), available: false }));
}
for (const tool of ['brush', 'eraser', 'rectangle', 'lasso']) assert.ok(allowed(tool, context('vector', {}, { layerEditTarget: 'mask' })));
assert.ok(!allowed('blur', context('paint', {}, { layerEditTarget: 'mask' })));
assert.ok(!allowed('brush', context('paint', { visible: false })));
assert.ok(!allowed('brush', { ...context(), busy: true }));
assert.ok(!allowed('brush', { ...context(), editable: false }));
assert.ok(!allowed('brush', context('paint', {}, { layerEditTarget: 'none' })));
assert.ok(!allowed('brush', context('paint', {}, { layers: [] })));
assert.ok(!allowed('brush', context('paint', {}, { layerGroups: { members: [], groups: [
  { id: 'inner', children: ['one'], locked: false, visible: true },
  { id: 'outer', children: ['inner'], locked: true, visible: true },
] } })));
for (const locale of ['en', 'ja', 'zh-CN']) assert.ok(toolUnavailableReason('vectorPen', context(), locale));
assert.equal(toolbarTool('transform'), 'vectorScale');
assert.equal(toolbarTool('selection'), 'rectangle');
console.log('Tool availability: layer types, masks, locks, groups, document state and locales passed.');
