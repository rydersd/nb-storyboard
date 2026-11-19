import assert from 'node:assert/strict';
import test from 'node:test';

import { freezeDocument } from '../js/core/document-freeze.js';
import { applyServer, state } from '../js/core/store.js';

function documentWithShots(count) {
  return {
    format: { width: 1920, height: 1080 },
    layout: { positions: {} },
    scenes: [{ id: 'scene-1', shots: Array.from({ length: count }, (_, index) => ({
      id: `shot-${index + 1}`,
      takes: [{ id: `take-${index + 1}`, panels: [{ id: `panel-${index + 1}`, layers: [{ id: `layer-${index + 1}`, type: 'image', src: 'sources/a.png' }] }] }],
    })) }],
  };
}

test('extension document view is deeply frozen within the 10 ms interaction budget', () => {
  const document = documentWithShots(300);
  const started = performance.now();
  const frozen = freezeDocument(document);
  const elapsed = performance.now() - started;

  assert.equal(frozen, document);
  assert.ok(elapsed < 10, `freezing 300 shots took ${elapsed.toFixed(2)} ms`);
  assert.ok(Object.isFrozen(document.scenes[0].shots[0].takes[0].panels[0].layers[0]));
  assert.throws(() => { document.scenes[0].shots[0].takes[0].panels[0].layers[0].src = 'sources/rewritten.png'; }, TypeError);
});

test('the page store exposes the frozen document to extensions', () => {
  const document = documentWithShots(1);
  applyServer(document, 'freeze-test', { initial: true });

  assert.equal(state.doc, document);
  assert.throws(() => { state.doc.scenes[0].shots[0].takes[0].panels[0].layers[0].src = 'sources/rewritten.png'; }, TypeError);
});
