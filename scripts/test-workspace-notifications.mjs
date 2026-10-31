import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
const code = stripTypeScriptTypes(readFileSync(new URL('../src/workspace-notifications.ts', import.meta.url), 'utf8'));
const { applyNotification } = await import('data:text/javascript;base64,' + Buffer.from(code).toString('base64'));
const a = { id: 'a', name: '日本語' }, b = { id: 'b', name: '简体中文' };
const workspace = objects => ({ activeId: 1, documents: [], active: { layers: [{ id: 'layer', objects }] } });
const full = { sequence: 1, baseSequence: null, workspace: workspace([a,b]), objectPatches: [] };
let state = applyNotification(undefined, full);
const delta = (sequence, patches) => ({ sequence, baseSequence: sequence - 1, workspace: workspace([]), objectPatches: patches });
state = applyNotification(state, delta(2, []));
assert.equal(state.workspace.active.layers[0].objects[0], a);
const changed = { ...b, name: 'English', strokeStyle: { miterLimit: 5 } };
state = applyNotification(state, delta(3, [{ layerId: 'layer', upsert: [changed], order: null }]));
assert.deepEqual(state.workspace.active.layers[0].objects, [a, changed]);
assert.equal(state.workspace.active.layers[0].objects[0], a);
state = applyNotification(state, delta(4, [{ layerId: 'layer', upsert: [], order: ['b'] }]));
assert.deepEqual(state.workspace.active.layers[0].objects, [changed]);
assert.equal(applyNotification(state, full), state);
assert.equal(applyNotification(state, delta(6, [])), undefined);
assert.equal(applyNotification(undefined, delta(2, [])), undefined);
assert.equal(applyNotification(state, delta(5, [{ layerId: 'layer', upsert: [], order: ['missing'] }])), undefined);
assert.equal(applyNotification(state, delta(5, [{ layerId: 'layer', upsert: [], order: ['b','b'] }])), undefined);
assert.equal(applyNotification(state, { ...delta(5, []), workspace: { ...workspace([]), activeId: 2 } }), undefined);
state = applyNotification(state, { ...full, sequence: 7 });
assert.deepEqual(state.workspace.active.layers[0].objects, [a,b]);
// New/deleted layers and an empty order replace exactly the authoritative list.
const added = { sequence: 8, baseSequence: 7, workspace: { ...workspace([]), active: { layers: [{ id: 'new', objects: [] }] } }, objectPatches: [{ layerId: 'new', upsert: [a], order: ['a'] }] };
state = applyNotification(state, added);
assert.deepEqual(state.workspace.active.layers.map(layer => layer.id), ['new']);
assert.deepEqual(state.workspace.active.layers[0].objects, [a]);
state = applyNotification(state, { ...added, sequence: 9, baseSequence: 8, objectPatches: [{ layerId: 'new', upsert: [], order: [] }] });
assert.deepEqual(state.workspace.active.layers[0].objects, []);
console.log('Workspace delta, identity, ordering, deletion, stale packet and recovery checks passed.');

// Search is view metadata: it must advance even when artwork revision is stable,
// while late packets cannot replace the current query or copy unchanged objects.
const searchPacket = (sequence, query, enabled = true) => ({
  sequence, baseSequence: sequence === 1 ? null : sequence - 1,
  workspace: { ...workspace([]), active: {
    ...workspace(sequence === 1 ? [a,b] : []).active, revision: 42,
    layerGroups: { filterEnabled: enabled, filterQuery: query,
      filter: { kind: 'vector', visible: null, locked: null },
      filterMatches: enabled ? ['layer'] : null, filterTargets: enabled ? ['layer'] : null,
      groups: [], roots: ['layer'], members: [], selected: ['layer'] },
  } }, objectPatches: [],
});
let searchState = applyNotification(undefined, searchPacket(1, ''));
const earlierQuery = searchPacket(2, 'Vec');
searchState = applyNotification(searchState, earlierQuery);
searchState = applyNotification(searchState, searchPacket(3, 'Vector'));
assert.equal(searchState.workspace.active.layerGroups.filterQuery, 'Vector');
assert.equal(searchState.workspace.active.revision, 42);
assert.equal(searchState.workspace.active.layers[0].objects[0], a);
assert.equal(applyNotification(searchState, earlierQuery), searchState);
searchState = applyNotification(searchState, searchPacket(4, '', false));
assert.equal(searchState.workspace.active.layerGroups.filterEnabled, false);
assert.equal(searchState.workspace.active.layerGroups.filterTargets, null);
assert.deepEqual(searchState.workspace.active.layerGroups.selected, ['layer']);
assert.equal(searchState.workspace.active.layers[0].objects[0], a);
console.log('Layer search metadata, stable revision, stale query rejection and selection retention checks passed.');
