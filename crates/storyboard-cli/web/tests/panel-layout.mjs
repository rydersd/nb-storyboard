import assert from 'node:assert/strict';
import test from 'node:test';

import { clampDockSize, layoutFor, movePanel, normalizeLayout, resizeDock } from '../js/core/panel-layout.js';

const ids = ['graph', 'inspector', 'stickies', 'history', 'sources', 'switcher', 'player', 'cut-editor', 'sample.dialogue-word-count'];

test('panel layout retains each known panel once and drops unknown saved ids', () => {
  const layout = normalizeLayout({
    left: ['sources', 'gone', 'sources'], right: ['inspector', 'sources'], bottom: ['player'], hidden: ['history'],
    sizes: { left: 12, right: 400, bottom: 9999, surprise: 300 },
  }, ids);
  assert.deepEqual(layout.left, ['sources']);
  assert.deepEqual(layout.right, ['inspector', 'graph', 'stickies', 'switcher', 'cut-editor', 'sample.dialogue-word-count']);
  assert.deepEqual(layout.bottom, ['player']);
  assert.deepEqual(layout.center, []);
  assert.deepEqual(layout.hidden, ['history']);
  assert.deepEqual(layout.sizes, { right: 400 });
});

test('moving a panel tab creates a single dock owner and preserves valid sizes', () => {
  const start = layoutFor(ids);
  const moved = movePanel(start, 'sources', 'left');
  const bottom = movePanel(moved, 'sources', 'bottom');
  const centered = movePanel(bottom, 'graph', 'center');
  assert.deepEqual(bottom.left, []);
  assert.deepEqual(bottom.bottom, ['sources']);
  assert.equal([...bottom.left, ...bottom.right, ...bottom.bottom, ...bottom.hidden].filter((id) => id === 'sources').length, 1);
  assert.deepEqual(centered.center, ['graph']);
});

test('splitter maths clamps docks before a panel can collapse to zero', () => {
  assert.equal(clampDockSize('left', 5), 160);
  assert.equal(clampDockSize('bottom', 2000), 800);
  assert.equal(resizeDock({ left: 300 }, 'left', -1000).left, 160);
  assert.equal(resizeDock({ bottom: 240 }, 'bottom', 1000).bottom, 800);
});
