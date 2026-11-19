// The keyboard list (?): gestures plus every registered command that names a keymap row.

import { els, h } from '../core/dom.js';
import { closePopover, currentPopover, openPopover } from '../core/popover.js';
import { getCommands, registerCommand, registerPanel } from '../core/registry.js';

const GESTURE_ROWS = [
  { order: 10, keys: 'Space+drag, middle-drag', label: 'Pan' },
  { order: 20, keys: 'Wheel, pinch, Cmd/Ctrl+wheel', label: 'Zoom at the pointer' },
  { order: 160, keys: 'Alt while dragging', label: 'Only move, never reorder' },
];

/** [keys, action] rows in order: the gestures above and each command's keymap entry. */
export function keymapRows() {
  const rows = [...GESTURE_ROWS, ...getCommands().filter((command) => command.keymap).map((command) => command.keymap)];
  return rows.sort((a, b) => a.order - b.order).map((row) => [row.keys, row.label]);
}

export function openKeymap() {
  if (currentPopover()?.keymap) { closePopover(); return; }
  const panel = h('div', { class: 'keymap', role: 'dialog', 'aria-label': 'Keyboard shortcuts', tabindex: '-1' },
    h('h2', { text: 'Keys' }),
    h('table', null, h('tbody', null, keymapRows().map(([keys, action]) => h('tr', null, h('td', { text: keys }), h('td', { text: action }))))));
  panel.addEventListener('keydown', (event) => { if (event.key === 'Escape' || event.key === '?') { event.preventDefault(); event.stopPropagation(); closePopover(); } });
  openPopover(panel, { center: true, anchor: els.keysButton, onClose: () => els.graph.focus({ preventScroll: true }) }).keymap = true;
  panel.focus({ preventScroll: true });
}

registerPanel({
  id: 'keymap', title: 'Keyboard shortcuts', region: 'status', order: 30,
  mount(el) {
    els.keysButton = h('button', { id: 'keys-button', class: 'icon-button', 'aria-label': 'Keyboard shortcuts', title: 'Keyboard shortcuts (?)', text: '?' });
    el.append(els.keysButton);
    els.keysButton.addEventListener('click', openKeymap);
    return {};
  },
});

registerCommand({
  id: 'help.keys', title: 'Keyboard shortcuts', keys: ['Mod?+?'], needsDoc: false, run: () => openKeymap(),
  keymap: { order: 210, keys: '?', label: 'This list' },
});
