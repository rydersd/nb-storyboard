import { commit } from '../core/api.js';
import { reorderIndex } from '../core/reorder.js';
import { registerCommand, registerPanel } from '../core/registry.js';
import { selectOnly, shotEntry, shotView, state } from '../core/store.js';

function operation(kind, ids, from, target, edge) {
  const index = reorderIndex(ids, from, target, edge);
  if (index < 0 || index === ids.indexOf(from)) return null;
  const shot = state.primary?.type === 'shot' ? state.primary.id : null;
  if (kind === 'panel') { const view = shot && shotView(shotEntry(shot).shot); return view && { move_panel: { shot, take: view.take.id, panel: from, index } }; }
  if (kind === 'take') return shot && { move_take: { shot, take: from, index } };
  return { move_scene: { scene: from, index } };
}

function keyboardMove(kind, delta) {
  const shot = state.primary?.type === 'shot' && shotEntry(state.primary.id)?.shot;
  const scene = state.primary?.type === 'scene' && state.primary.id;
  const view = shot && shotView(shot);
  const ids = kind === 'panel' ? view?.take?.panels.map((item) => item.id)
    : kind === 'take' ? shot?.takes.map((item) => item.id) : state.doc?.scenes.map((item) => item.id);
  const id = kind === 'panel' ? view?.panel?.id : kind === 'take' ? view?.take?.id : scene;
  const from = ids?.indexOf(id), target = ids?.[from + delta];
  if (!target) return;
  const op = operation(kind, ids, id, target, delta < 0 ? 'before' : 'after');
  if (op) commit([op], { what: `Reorder ${kind}` });
}

for (const [kind, keys, label] of [
  ['panel', ['Alt+Shift+ArrowLeft', 'Alt+Shift+ArrowRight'], 'Move selected panel earlier or later'],
  ['take', ['Alt+Shift+ArrowUp', 'Alt+Shift+ArrowDown'], 'Move viewed take earlier or later'],
  ['scene', ['Alt+Shift+ArrowLeft', 'Alt+Shift+ArrowRight'], 'Move selected scene earlier or later'],
]) {
  registerCommand({ id: `reorder.${kind}`, title: label, keys,
    when: () => kind === 'scene' ? state.primary?.type === 'scene' : state.primary?.type === 'shot',
    run: (_, event) => keyboardMove(kind, event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 1),
    keymap: { order: 132, keys: keys.map((key) => key.replaceAll('Arrow', '')).join(' / '), label },
  });
}

registerPanel({
  id: 'reorder', title: 'Reorder controls', region: 'graph', order: 20,
  mount(el) {
    let drag = null;
    const clear = () => { document.querySelectorAll('[data-reorder-edge]').forEach((node) => { delete node.dataset.reorderEdge; node.classList.remove('reorder-before', 'reorder-after'); }); drag = null; };
    const down = (event) => {
      const item = event.target.closest('[data-reorder-kind]');
      if (!item || event.button !== 0) return;
      const kind = item.dataset.reorderKind, id = item.dataset.reorderId;
      const ids = kind === 'panel' ? [...item.parentElement.querySelectorAll('[data-reorder-kind="panel"]')].map((node) => node.dataset.reorderId)
        : kind === 'take' ? [...item.parentElement.querySelectorAll('[data-reorder-kind="take"]')].map((node) => node.dataset.reorderId)
          : state.doc.scenes.map((scene) => scene.id);
      event.preventDefault(); event.stopPropagation(); item.setPointerCapture(event.pointerId); drag = { kind, id, ids, item, pointerId: event.pointerId, target: null, edge: 'before' };
    };
    const move = (event) => {
      if (!drag || event.pointerId !== drag.pointerId) return;
      const target = document.elementFromPoint(event.clientX, event.clientY)?.closest(`[data-reorder-kind="${drag.kind}"]`);
      clearMarks();
      if (!target || target.dataset.reorderId === drag.id) return;
      const box = target.getBoundingClientRect();
      const edge = drag.kind === 'take' ? (event.clientX < box.left + box.width / 2 ? 'before' : 'after') : (event.clientY < box.top + box.height / 2 ? 'before' : 'after');
      target.dataset.reorderEdge = edge; target.classList.add(`reorder-${edge}`); drag.target = target.dataset.reorderId; drag.edge = edge;
    };
    const clearMarks = () => document.querySelectorAll('[data-reorder-edge]').forEach((node) => { delete node.dataset.reorderEdge; node.classList.remove('reorder-before', 'reorder-after'); });
    const up = (event) => {
      if (!drag || event.pointerId !== drag.pointerId) return;
      const { kind, ids, id, target, edge } = drag; clearMarks(); drag = null;
      if (!target) return;
      if (kind === 'scene') selectOnly('scene', id);
      const op = operation(kind, ids, id, target, edge);
      if (op) commit([op], { what: `Reorder ${kind}` });
    };
    document.addEventListener('pointerdown', down, true); document.addEventListener('pointermove', move, true); document.addEventListener('pointerup', up, true);
    return { unmount: () => { document.removeEventListener('pointerdown', down, true); document.removeEventListener('pointermove', move, true); document.removeEventListener('pointerup', up, true); clear(); } };
  },
});
