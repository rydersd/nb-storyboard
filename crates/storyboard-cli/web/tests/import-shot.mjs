import assert from 'node:assert/strict';
import test from 'node:test';
import { imageSelectionError, insertionAfterShot, MAX_SHOT_IMAGE_BYTES } from '../js/core/import-shot.js';

const file = (name, size = 10) => ({ name, size });

test('image sequence preflight catches server limits before upload', () => {
  assert.equal(imageSelectionError([file('frame-1.png'), file('frame-2.png')]), '');
  assert.match(imageSelectionError(Array.from({ length: 501 }, (_, i) => file(`${i}.png`))), /1 to 500/);
  assert.match(imageSelectionError([file('frame.png', MAX_SHOT_IMAGE_BYTES + 1)]), /frame.png.*32 MB/);
  assert.match(imageSelectionError([file('frame.png', 0)]), /frame.png.*32 MB/);
  assert.match(imageSelectionError([file('A.png'), file('a.png')]), /duplicate/);
});

test('insertion follows the selected shot after a concurrent reorder', () => {
  const doc = { scenes: [{ id: 'scene-1', shots: [{ id: 'other' }, { id: 'anchor' }, { id: 'last' }] }] };
  assert.equal(insertionAfterShot(doc, 'scene-1', 'anchor'), 2);
  doc.scenes[0].shots.unshift(doc.scenes[0].shots.splice(1, 1)[0]);
  assert.equal(insertionAfterShot(doc, 'scene-1', 'anchor'), 1);
  assert.equal(insertionAfterShot(doc, 'scene-1', null), null);
  doc.scenes[0].shots.shift();
  assert.throws(() => insertionAfterShot(doc, 'scene-1', 'anchor'), /moved or was removed/);
});
