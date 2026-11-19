import assert from 'node:assert/strict';
import test from 'node:test';

import { dialogueWordCount } from '../extensions/dialogue-word-count.js';

test('sample dialogue extension counts words per scene', () => {
  const document = { scenes: [{ id: 'a', shots: [{ dialogue: 'One, two.' }, { dialogue: 'Three' }] }] };
  assert.equal(dialogueWordCount(document, 'a'), 3);
});
