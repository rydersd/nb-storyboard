import assert from 'node:assert/strict';
import test from 'node:test';

import { hiddenGate } from '../js/core/panel-catchup.js';

test('a hidden panel skips store events and replays the latest of each, in order, when opened', () => {
  let hidden = false;
  const seen = [];
  const gate = hiddenGate(() => hidden, (call) => call());
  const onDoc = gate.wrap((payload) => seen.push(['doc', payload]));
  const onSelection = gate.wrap((payload) => seen.push(['selection', payload]));
  onDoc(1);
  hidden = true;
  onDoc(2); onSelection('a'); onDoc(3);
  assert.deepEqual(seen, [['doc', 1]]);
  hidden = false;
  const replayed = gate.flush();
  assert.deepEqual(seen, [['doc', 1], ['selection', 'a'], ['doc', 3]]); // in the order of each one's latest event
  assert.equal(replayed.size, 2);
  assert.equal(gate.flush().size, 0);
});

test('replays run through the panel guard', () => {
  const guarded = [];
  const gate = hiddenGate(() => true, (call) => { guarded.push('guard'); return call(); });
  gate.wrap(() => {})('x');
  assert.deepEqual(guarded, []);
  gate.flush();
  assert.deepEqual(guarded, ['guard']);
});
