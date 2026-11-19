import assert from 'node:assert/strict';
import test from 'node:test';

import { comboScore } from '../js/core/commands.js';

test('panel digit shortcut uses the physical Digit code when macOS Option changes event.key', () => {
  const event = { key: '¡', code: 'Digit1', metaKey: true, ctrlKey: false, altKey: true, shiftKey: false };
  assert.equal(comboScore('Mod+Alt+1', event), 2);
});
