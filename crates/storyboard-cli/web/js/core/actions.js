// Shot and scene actions shared by keys, menus and buttons, and the commands that run them.

import { commit, optimisticCommit, undo } from './api.js';
import { nextId, shotStatus, STATUSES } from './model.js';
import { registerCommand } from './registry.js';
import { clearSelection, emit, notify, selectAllShots, selectOnly, shotEntry, state } from './store.js';
import { clamp } from './util.js';

export function addShotToScene(sceneId, index) {
  const entry = state.model.scenes.get(sceneId);
  if (!entry) return;
  commit([{ add_shot: { scene: sceneId, title: '', index: index ?? entry.scene.shots.length } }], {
    what: 'Add shot',
    onDone: (result) => focusNewShot(result),
  });
}
export function focusNewShot(result) {
  const shotId = (result.ids || []).find((id) => id.startsWith('shot-') && state.model.shots.has(id));
  if (!shotId) return;
  selectOnly('shot', shotId);
  emit('focusTitle', shotId);
}

/** + Shot / A: after the selected shot, or at the end of the selected (or last) scene. */
export function addShot() {
  if (!state.doc) return;
  const primary = state.primary;
  if (primary?.type === 'shot' && state.model.shots.has(primary.id)) {
    const entry = state.model.shots.get(primary.id);
    return addShotToScene(entry.scene.id, entry.index + 1);
  }
  if (primary?.type === 'scene') return addShotToScene(primary.id, null);
  const last = state.doc.scenes[state.doc.scenes.length - 1];
  if (last) return addShotToScene(last.id, null);
  const sceneId = nextId('scene', []);
  commit([{ add_scene: { id: sceneId, title: '' } }, { add_shot: { scene: sceneId, title: '' } }], { what: 'Add shot', onDone: focusNewShot });
}

/** Shift+A: a new scene after the selected one (or last), title in edit. */
export function addScene() {
  if (!state.doc) return;
  let index = state.doc.scenes.length;
  const primary = state.primary;
  const sceneId = primary?.type === 'scene' ? primary.id : primary?.type === 'shot' ? state.model.shots.get(primary.id)?.scene.id : null;
  if (sceneId && state.model.scenes.has(sceneId)) index = state.model.scenes.get(sceneId).index + 1;
  commit([{ add_scene: { title: '', index } }], {
    what: 'Add scene',
    onDone: (result) => {
      const id = (result.ids || []).find((candidate) => state.model.scenes.has(candidate));
      if (!id) return;
      selectOnly('scene', id);
      emit('renameScene', id);
    },
  });
}

export function removeSelection() {
  const ops = [];
  const names = [];
  for (const id of state.sel.shots) { ops.push({ remove_shot: { shot: id } }); names.push(id); }
  for (const id of state.sel.stickies) { ops.push({ remove_sticky: { sticky: id } }); names.push(id); }
  if (!ops.length && state.sel.scene) { ops.push({ remove_scene: { scene: state.sel.scene } }); names.push(state.sel.scene); }
  if (!ops.length) return;
  const label = `removed ${names.length > 3 ? `${names.length} items` : names.join(', ')}. Cmd+Z to undo`;
  commit(ops, { what: 'Remove', label, onDone: () => clearSelection() });
}

export function setStatusFor(ids, status) {
  const ops = ids.filter((id) => state.model.shots.has(id) && shotStatus(state.model.shots.get(id).shot) !== status)
    .map((id) => ({ update_shot: { shot: id, patch: { status } } }));
  if (ops.length) commit(ops, { what: 'Status' });
}

/** Shift+T: clear stored positions so the nodes return to their auto-layout slots. */
export function tidySelection() {
  const positions = state.doc.layout.positions;
  const keys = [...[...state.sel.shots].map((id) => `shot:${id}`), ...[...state.sel.stickies].map((id) => `sticky:${id}`)].filter((key) => positions[key]);
  if (!keys.length) { notify.status({ text: state.sel.shots.size || state.sel.stickies.size ? 'Already in auto-layout.' : 'Select the nodes to tidy first.' }); return; }
  optimisticCommit(keys.map((node) => ({ move_node: { node, position: null } })), { what: 'Tidy', label: `tidied ${keys.length} node${keys.length === 1 ? '' : 's'}` });
}

/** Alt+Left/Right: earlier or later by one, crossing into the neighbouring scene at its edge. */
export function nudgeOrder(delta) {
  const entry = state.primary?.type === 'shot' && shotEntry(state.primary.id);
  if (!entry || state.sel.shots.size !== 1) return;
  const { scene, index, sceneIndex, shot } = entry;
  const target = index + delta;
  let op;
  if (target >= 0 && target < scene.shots.length) op = { move_shot: { shot: shot.id, index: target } };
  else {
    const neighbour = state.doc.scenes[sceneIndex + delta];
    if (!neighbour) return;
    op = { move_shot_between_scenes: { shot: shot.id, scene: neighbour.id, index: delta < 0 ? neighbour.shots.length : 0 } };
  }
  optimisticCommit([op], { what: 'Reorder' });
}

export function moveSelection(direction) {
  const model = state.model;
  if (!model.order.length) return;
  const current = state.primary?.type === 'shot' ? model.shots.get(state.primary.id) : null;
  let next = null;
  if (!current) next = model.order[0];
  else if (direction === 'left' || direction === 'right') {
    const index = model.order.indexOf(current.shot.id) + (direction === 'right' ? 1 : -1);
    next = model.order[clamp(index, 0, model.order.length - 1)];
  } else {
    const step = direction === 'down' ? 1 : -1;
    const from = state.layout.shots.get(current.shot.id);
    for (let sceneIndex = current.sceneIndex + step; sceneIndex >= 0 && sceneIndex < state.doc.scenes.length; sceneIndex += step) {
      const shots = state.doc.scenes[sceneIndex].shots;
      if (!shots.length) continue;
      next = shots.reduce((best, shot) => {
        const p = state.layout.shots.get(shot.id);
        const b = state.layout.shots.get(best.id);
        return Math.abs(p.x - from.x) < Math.abs(b.x - from.x) ? shot : best;
      }).id;
      break;
    }
  }
  if (!next) return;
  selectOnly('shot', next, { focus: true });
  emit('reveal', next);
}

registerCommand({
  id: 'edit.undo', title: 'Undo my last change', keys: ['Mod+z'], when: (ctx, event) => !event?.shiftKey,
  run: () => undo(), keymap: { order: 170, keys: 'Cmd/Ctrl+Z', label: 'Undo my last change' },
});
registerCommand({
  id: 'selection.all', title: 'Select all shots', keys: ['Mod+a'], run: () => selectAllShots(),
  keymap: { order: 80, keys: 'Shift+click, Cmd/Ctrl+A', label: 'Add to the selection, select all' },
});
registerCommand({
  id: 'shot.add', title: 'Add shot', keys: ['a'], run: () => addShot(),
  keymap: { order: 90, keys: 'A / Shift+A', label: 'Add shot, add scene' },
});
registerCommand({ id: 'scene.add', title: 'Add scene', keys: ['A'], run: () => addScene() });
registerCommand({
  id: 'selection.remove', title: 'Remove the selection', keys: ['Delete', 'Backspace'], run: () => removeSelection(),
  keymap: { order: 140, keys: 'Delete, Backspace', label: 'Remove the selection' },
});
registerCommand({
  id: 'shot.status', title: 'Set status', keys: ['1', '2', '3', '4'],
  run: (ctx, event) => { if (state.sel.shots.size && event) setStatusFor([...state.sel.shots], STATUSES[Number(event.key) - 1]); },
  keymap: { order: 110, keys: '1 2 3 4', label: 'Status: draft, review, approved, changes' },
});
registerCommand({
  id: 'layout.tidy', title: 'Tidy to auto-layout', keys: ['Shift+T'], run: () => tidySelection(),
  keymap: { order: 150, keys: 'Shift+T', label: 'Tidy back to auto-layout' },
});
registerCommand({
  id: 'shot.nudge', title: 'Move the shot earlier or later', keys: ['Alt+ArrowLeft', 'Alt+ArrowRight'],
  run: (ctx, event) => nudgeOrder(event?.key === 'ArrowLeft' ? -1 : 1),
  keymap: { order: 70, keys: 'Alt+Left / Alt+Right', label: 'Move the selected shot earlier or later' },
});
registerCommand({
  id: 'selection.step', title: 'Previous or next shot', keys: ['ArrowLeft', 'ArrowRight'],
  run: (ctx, event) => moveSelection(event?.key === 'ArrowLeft' ? 'left' : 'right'),
  keymap: { order: 50, keys: 'Left / Right', label: 'Previous or next shot in story order' },
});
registerCommand({
  id: 'selection.row', title: 'Nearest shot above or below', keys: ['ArrowUp', 'ArrowDown'],
  run: (ctx, event) => moveSelection(event?.key === 'ArrowUp' ? 'up' : 'down'),
  keymap: { order: 60, keys: 'Up / Down', label: 'Nearest shot in the scene above or below' },
});
