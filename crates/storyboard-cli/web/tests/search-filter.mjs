import assert from 'node:assert/strict';
import test from 'node:test';
import { performance } from 'node:perf_hooks';

import { filterShotIds } from '../js/core/search-filter.js';

const model = {
  order: ['one', 'two', 'three'],
  shots: new Map([
    ['one', { shot: { id: 'one', title: 'Arrival', status: 'draft', action: 'A car stops.', dialogue: 'Hello', notes: 'rain' } }],
    ['two', { shot: { id: 'two', title: 'Departure', status: 'approved', action: 'A door closes.', dialogue: '', notes: '' } }],
    ['three', { shot: { id: 'three', title: 'Closeup', status: 'draft', action: '', dialogue: '', notes: '' } }],
  ]),
  pinnedToShot: new Map([['three', [{ id: 'sticky-1', resolved: false }]]]),
};

test('search filters title, action, dialogue, notes, status, and unresolved stickies together', () => {
  assert.deepEqual([...filterShotIds(model, { query: 'hello' })], ['one']);
  assert.deepEqual([...filterShotIds(model, { query: 'door', status: 'approved' })], ['two']);
  assert.deepEqual([...filterShotIds(model, { unresolved: true, status: 'draft' })], ['three']);
});

test('searching a generated 300-shot project stays below the interaction budget', () => {
  const shots = new Map();
  const order = [];
  for (let index = 0; index < 300; index += 1) {
    const id = `shot-${index}`;
    order.push(id);
    shots.set(id, { shot: { id, title: `Shot ${index}`, status: index % 2 ? 'draft' : 'approved', action: `Action ${index}`, dialogue: '', notes: '' } });
  }
  const started = performance.now();
  const matches = filterShotIds({ order, shots, pinnedToShot: new Map() }, { query: 'action', status: 'draft' });
  const elapsed = performance.now() - started;

  assert.equal(matches.size, 150);
  assert.ok(elapsed < 60, `search took ${elapsed.toFixed(2)}ms`);
});
