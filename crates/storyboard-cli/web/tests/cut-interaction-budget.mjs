import assert from 'node:assert/strict';
import test from 'node:test';

import { cutDropTarget } from '../js/core/cut-interaction.js';

function p95(samples) { return samples.sort((a, b) => a - b)[Math.floor(samples.length * 0.95)]; }
for (const count of [18, 300]) test(`cut interaction target stays below 60 ms p95 at ${count} shots`, () => {
  const items = Array.from({ length: count }, (_, index) => `clip-${index}`);
  const samples = [];
  for (let index = 0; index < 1000; index++) {
    const start = performance.now();
    assert.equal(cutDropTarget(items, index % count, (index * 7) % count), (index * 7) % count);
    samples.push(performance.now() - start);
  }
  assert.ok(p95(samples) < 60, `p95 ${p95(samples).toFixed(3)} ms`);
});
