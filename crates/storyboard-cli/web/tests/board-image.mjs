import assert from 'node:assert/strict';
import test from 'node:test';
import { boardImageOp } from '../js/core/board-image.js';

const format = { width: 1920, height: 1080 };
const ids = ['shot-1', 'take-1'];

test('replacing an imported board changes its source without removing overlay layers', () => {
  const panel = { id: 'panel-1', layers: [
    { id: 'board', type: 'image', x: 0, y: 0, width: 1920, height: 1080, src: 'old.png' },
    { id: 'caption', type: 'text', x: 10, y: 10, width: 500, height: 100, text: 'Keep me' },
  ] };
  assert.deepEqual(boardImageOp(...ids, panel, format, 'new.png'), {
    update_layer: { shot: 'shot-1', take: 'take-1', panel: 'panel-1', layer: 'board', patch: { src: 'new.png' } },
  });
});

test('a board is added underneath existing art when no full-canvas image exists', () => {
  const panel = { id: 'panel-2', layers: [
    { id: 'inset', type: 'image', x: 20, y: 20, width: 300, height: 200, src: 'inset.png' },
  ] };
  assert.deepEqual(boardImageOp(...ids, panel, format, 'new.png'), {
    add_layer: { shot: 'shot-1', take: 'take-1', panel: 'panel-2', layer_type: 'image',
      fields: { src: 'new.png', fit: 'contain' }, index: 0 },
  });
});
