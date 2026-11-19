import { redo } from './api.js';
import { registerCommand } from './registry.js';

export const redoKeyMatches = (event) => !!(event.shiftKey && (event.metaKey || event.ctrlKey) && String(event.key).toLowerCase() === 'z');

registerCommand({
  id: 'history.redo', title: 'Redo my last undone change', keys: ['Mod+Shift+z'],
  run: () => redo(),
  keymap: { order: 101, keys: 'Shift+Cmd/Ctrl+Z', label: 'Redo my last undone change' },
});
