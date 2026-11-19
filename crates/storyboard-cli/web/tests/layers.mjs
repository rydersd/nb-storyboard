import assert from 'node:assert/strict';
import test from 'node:test';

import { layerDragPatch, layerHit } from '../js/core/layer-edit.js';

test('layer hit testing chooses the topmost containing layer', () => {
  const layers = [
    { id: 'base', x: 5, y: 5, width: 80, height: 80 },
    { id: 'top', x: 20, y: 20, width: 30, height: 30 },
  ];

  assert.equal(layerHit(layers, 25, 25)?.id, 'top');
  assert.equal(layerHit(layers, 10, 10)?.id, 'base');
  assert.equal(layerHit(layers, 95, 95), null);
});

test('layer drag patches move and southeast resize geometry without mutating the source', () => {
  const layer = { id: 'caption', x: 10, y: 20, width: 30, height: 40 };

  assert.deepEqual(layerDragPatch(layer, 'move', 4, -7), { x: 14, y: 13 });
  assert.deepEqual(layerDragPatch(layer, 'se', 8, 6), { width: 38, height: 46 });
  assert.deepEqual(layer, { id: 'caption', x: 10, y: 20, width: 30, height: 40 });
});
