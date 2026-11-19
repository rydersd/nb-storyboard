// Keyboard dispatch over the registered commands (see registerCommand in registry.js).

import { isTextTarget } from './dom.js';
import { closePopover, isPopoverOpen } from './popover.js';
import { context, getCommand, getCommands, guard } from './registry.js';
import { notify, state } from './store.js';

const keyFilters = [];

/**
 * A key filter sees keys before commands and returns true to consume them (a drag, an overlay).
 * beforeText filters run even while a text field has focus.
 */
export function addKeyFilter(fn, { beforeText = false } = {}) {
  keyFilters.push({ fn, beforeText });
}

const parsed = new Map();
function parseCombo(combo) {
  let spec = parsed.get(combo);
  if (spec) return spec;
  const parts = combo.split('+');
  const key = parts.pop();
  spec = { key: key === 'Space' ? ' ' : key, mod: false, alt: false, shift: false, weight: 0 };
  for (const part of parts) {
    if (part === 'Mod') { spec.mod = true; spec.weight++; }
    else if (part === 'Mod?') spec.mod = null;
    else if (part === 'Alt') { spec.alt = true; spec.weight++; }
    else if (part === 'Shift') { spec.shift = true; spec.weight++; }
    else throw new Error(`unknown modifier ${part} in ${combo}`);
  }
  parsed.set(combo, spec);
  return spec;
}

/** How well an event matches a combo: -1 for no match, else the number of modifiers it named. */
export function comboScore(combo, event) {
  const spec = parseCombo(combo);
  const mod = event.metaKey || event.ctrlKey;
  if (spec.mod !== null && spec.mod !== mod) return -1;
  if (spec.alt && !event.altKey) return -1;
  if (spec.shift && !event.shiftKey) return -1;
  // macOS Option changes key to glyphs such as \u00a1, but physical panel digits retain Digit1..Digit9.
  const matches = spec.alt && /^[1-9]$/.test(spec.key) && event.code
    ? event.code === `Digit${spec.key}`
    : spec.mod ? event.key.toLowerCase() === spec.key.toLowerCase() : event.key === spec.key;
  return matches ? spec.weight : -1;
}

function findCommand(event, global) {
  let best = null;
  let bestScore = -1;
  for (const command of getCommands()) {
    if (command.global !== global) continue;
    for (const combo of command.keys) {
      const score = comboScore(combo, event);
      if (score > bestScore && (!command.when || guard(`Command "${command.id}"`, null, () => command.when(context(), event)))) {
        best = command;
        bestScore = score;
      }
    }
  }
  return best;
}

function execute(command, event) {
  guard(`Command "${command.id}"`, null, () => command.run(context(), event), (error) => {
    notify.error({ message: `${command.title} failed: ${error.message}` }, null);
  });
}

/** Runs a registered command by id (toolbar buttons, menus). Returns false for an unknown id. */
export function runCommand(id, event = null) {
  const command = getCommand(id);
  if (!command) return false;
  execute(command, event);
  return true;
}

function onKeyDown(event) {
  const global = findCommand(event, true);
  if (global) { event.preventDefault(); execute(global, event); return; }
  for (const filter of keyFilters) if (filter.beforeText && filter.fn(event)) return;
  if (isTextTarget(event.target)) return;
  if (isPopoverOpen()) { if (event.key === 'Escape') { event.preventDefault(); closePopover(); } return; }
  for (const filter of keyFilters) if (!filter.beforeText && filter.fn(event)) return;
  const command = findCommand(event, false);
  if (!command || (command.needsDoc && !state.doc)) return;
  event.preventDefault();
  execute(command, event);
}

export function installKeyboard() {
  document.addEventListener('keydown', onKeyDown);
}
