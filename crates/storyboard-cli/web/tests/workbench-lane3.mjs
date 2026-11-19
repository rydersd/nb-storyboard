import assert from 'node:assert/strict';
import test from 'node:test';

import { layoutFor, movePanel, normalizeLayout } from '../js/core/panel-layout.js';
import { historyShotId } from '../js/core/history-map.js';
import { redoKeyMatches } from '../js/core/redo.js';
import { sourceImageLayerOp, sourceRefOps } from '../js/core/source-browser.js';
import { fmtClock } from '../js/core/util.js';
import { viewportPayload } from '../js/core/viewport-cache.js';

test('layout keeps every built-in panel once and moves a tab between docks', () => {
  const defaults = layoutFor(['graph', 'inspector', 'notes', 'switcher']);
  const moved = movePanel(defaults, 'notes', 'left');
  assert.deepEqual(moved.left, ['notes']);
  assert.ok(moved.right.includes('inspector'));
  assert.equal(new Set(['left', 'right', 'bottom', 'center', 'hidden'].flatMap((dock) => moved[dock])).size, 4);
  assert.deepEqual(normalizeLayout({ left: ['unknown', 'graph'], right: ['graph'] }, ['graph']), {
    left: ['graph'], right: [], bottom: [], center: [], hidden: [], sizes: {},
  });
});

test('history prefers a current shot footprint over a vague summary', () => {
  const doc = { shots: [{ id: 'shot-12', title: 'Arrival' }] };
  assert.equal(historyShotId({ summary: 'updated notes', footprints: ['shot/shot-12/notes'] }, doc), 'shot-12');
  assert.equal(historyShotId({ summary: 'updated notes', footprints: ['panel/shot-12/take-a/panel-a/layer/x'] }, doc), 'shot-12');
});

test('redo key accepts shift-mod-z only', () => {
  assert.equal(redoKeyMatches({ key: 'z', shiftKey: true, metaKey: true }), true);
  assert.equal(redoKeyMatches({ key: 'z', shiftKey: false, ctrlKey: true }), false);
});

test('source reference operations attach and detach through public ops', () => {
  assert.deepEqual(sourceRefOps({ kind: 'shot', shot: 'shot-1' }, 'sources/a.png', true), [{ add_ref: { target: { shot: { shot: 'shot-1' } }, reference: 'sources/a.png' } }]);
  assert.deepEqual(sourceRefOps({ kind: 'take', shot: 'shot-1', take: 'take-a' }, 'sources/a.png', false), [{ remove_ref: { target: { take: { shot: 'shot-1', take: 'take-a' } }, reference: 'sources/a.png' } }]);
});

test('source image drops add an image layer instead of replacing board art', () => {
  assert.deepEqual(sourceImageLayerOp('shot-1', 'take-a', 'panel-1', 'sources/a.png'), {
    add_layer: { shot: 'shot-1', take: 'take-a', panel: 'panel-1', layer_type: 'image',
      fields: { src: 'sources/a.png', fit: 'contain' } },
  });
});

test('project switcher duration is always a m:ss clock', () => {
  assert.equal(fmtClock(24 * 7, 24), '0:07');
  assert.equal(fmtClock(24 * 67, 24), '1:07');
});

test('viewport payload uses the server cache shape', () => {
  assert.deepEqual(viewportPayload({ x: 12, y: -8, z: 1.25 }, 'cut-a'), {
    pan_x: 12, pan_y: -8, zoom: 1.25, viewed_cut: 'cut-a',
  });
});
