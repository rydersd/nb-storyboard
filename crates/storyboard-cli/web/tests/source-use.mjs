import assert from 'node:assert/strict';
import test from 'node:test';

import { sourceRefOps, sourceUseFor } from '../js/core/source-browser.js';

const source = (...targets) => ({ path: 'sources/a.png', used_by: targets.map((target) => ({ target })) });

test('a source attached to shot-10 is not reported as used by shot-1', () => {
  assert.equal(sourceUseFor(source('take:shot-10/take-10', 'shot:shot-10'), { kind: 'take', shot: 'shot-1', take: 'take-1' }), null);
});

// RefTarget::Shot is a struct variant on the server: {"shot":{"shot":ID}} (storyboard-core ops.rs).
test('the use found for the selection is the exact ref target, so Detach removes that ref', () => {
  const selection = { kind: 'take', shot: 'shot-5', take: 'take-5' };
  assert.deepEqual(sourceUseFor(source('take:shot-5/take-5'), selection), { kind: 'take', shot: 'shot-5', take: 'take-5' });
  const shotLevel = sourceUseFor(source('shot:shot-5'), selection);
  assert.deepEqual(shotLevel, { kind: 'shot', shot: 'shot-5' });
  assert.deepEqual(sourceRefOps(shotLevel, 'sources/a.png', false), [{ remove_ref: { target: { shot: { shot: 'shot-5' } }, reference: 'sources/a.png' } }]);
});

test('another take of the same shot does not count as this take using the source', () => {
  assert.equal(sourceUseFor(source('take:shot-5/take-9'), { kind: 'take', shot: 'shot-5', take: 'take-5' }), null);
});

test('a layer use remains readable without masquerading as an attachable ref', () => {
  const layer = 'layer:shot-5/take-5/panel-5/layer-5';
  assert.equal(sourceUseFor(source(layer), { kind: 'take', shot: 'shot-5', take: 'take-5' }), null);
});
