import assert from 'node:assert/strict';
import test from 'node:test';

import { reorderIndex } from '../js/core/reorder.js';

test('reorder index accounts for removing the dragged item first', () => {
  assert.equal(reorderIndex(['a', 'b', 'c', 'd'], 'a', 'c', 'before'), 1);
  assert.equal(reorderIndex(['a', 'b', 'c', 'd'], 'd', 'b', 'after'), 2);
});

test('reorder index preserves a no-op drop', () => {
  assert.equal(reorderIndex(['a', 'b', 'c'], 'b', 'c', 'before'), 1);
});
