// Edit an intercut without changing the source scene or shot order.

import { commit } from '../core/api.js';
import { cutDropTarget } from '../core/cut-interaction.js';
import { h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { state } from '../core/store.js';
import { viewCut } from './paths.js';

registerPanel({
  id: 'cut-editor', title: 'Cut editor', region: 'toolbar', order: 34, dock: { defaultDock: 'bottom', shortcut: 'Mod+Alt+8' },
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'Edit cut' });
    const dialog = h('div', { class: 'cut-dialog', role: 'dialog', 'aria-label': 'Cut editor', hidden: true });
    el.append(button, dialog);
    let open = false;
    const edit = (ops, what, options = {}) => commit(ops, { what, ...options });
    const render = () => {
      button.hidden = !state.doc;
      if (!open || !state.doc) return;
      // Live project updates must not replace a focused, uncommitted timing field.
      if (dialog.contains(document.activeElement) && document.activeElement.matches('input[data-preserve-draft]')) return;
      const doc = state.doc;
      const cut = doc.cuts.find((item) => item.id === state.viewCut) || null;
      const choose = h('select', { 'aria-label': 'Cut' },
        h('option', { value: '', text: 'Choose a cut' }),
        ...doc.cuts.map((item) => h('option', { value: item.id, text: item.name || item.id })));
      choose.value = cut?.id || '';
      choose.addEventListener('change', () => viewCut(choose.value || null));
      const createName = h('input', { type: 'text', required: true, maxlength: 200, placeholder: 'New cut name', 'aria-label': 'New cut name' });
      const create = h('form', { class: 'cut-row' }, createName, h('button', { class: 'btn', type: 'submit', text: 'Create cut' }));
      create.addEventListener('submit', async (event) => {
        event.preventDefault();
        const name = createName.value.trim();
        if (!name) return;
        await commit([{ add_cut: { name } }], { what: 'Create cut', onDone: (result) => {
          const id = (result.ids || []).find((item) => item.startsWith('cut-'));
          if (id) viewCut(id);
        } });
      });
      const close = h('button', { class: 'btn', type: 'button', text: 'Close' });
      close.addEventListener('click', () => { open = false; dialog.hidden = true; });
      dialog.replaceChildren(h('div', { class: 'cut-row' }, h('strong', { text: 'Cut editor' }), choose, close), create);
      if (!cut) { dialog.append(h('p', { text: 'Create or choose a cut to interleave shots from any scene.' })); return; }
      const rename = h('input', { type: 'text', value: cut.name, maxlength: 200, 'aria-label': 'Cut name' });
      rename.dataset.preserveDraft = 'true';
      let renameBase = state.version;
      rename.addEventListener('focus', () => { renameBase = state.version; });
      rename.addEventListener('change', () => { if (rename.value.trim() !== cut.name) edit([{ update_cut: { cut: cut.id, patch: { name: rename.value.trim() } } }], 'Rename cut', { base: renameBase, strictBase: true, onFail: render }); });
      const active = h('button', { class: 'btn', type: 'button', text: doc.active_cut === cut.id ? 'Active cut' : 'Make active' });
      active.disabled = doc.active_cut === cut.id;
      active.addEventListener('click', () => edit([{ set_active_cut: { cut: cut.id } }], 'Make cut active'));
      const remove = h('button', { class: 'btn', type: 'button', text: 'Delete cut' });
      remove.addEventListener('click', async () => { await edit([{ remove_cut: { cut: cut.id } }], 'Delete cut'); viewCut(null); });
      dialog.append(h('div', { class: 'cut-row' }, rename, active, remove));
      const shotSelect = h('select', { 'aria-label': 'Shot to add' },
        ...doc.scenes.flatMap((scene) => scene.shots.map((shot) => h('option', { value: shot.id, text: `${scene.title || scene.id} · ${shot.title || shot.id}` }))));
      if (state.primary?.type === 'shot') shotSelect.value = state.primary.id;
      const add = h('button', { class: 'btn primary', type: 'button', text: 'Add shot to cut' });
      add.disabled = !shotSelect.options.length;
      add.addEventListener('click', () => edit([{ add_clip: { cut: cut.id, source: { shot: { shot: shotSelect.value } } } }], 'Add shot to cut'));
      dialog.append(h('div', { class: 'cut-row' }, shotSelect, add));
      const list = h('ol', { class: 'cut-clips' });
      for (const [index, clip] of cut.clips.entries()) {
        const source = clip.source.shot;
        const shot = source && doc.scenes.flatMap((scene) => scene.shots).find((item) => item.id === source.shot);
        const scene = shot && doc.scenes.find((item) => item.shots.some((candidate) => candidate.id === shot.id));
        const label = source ? `${scene?.title || scene?.id || 'Scene'} · ${shot?.title || source.shot}` : `Nested cut ${clip.source.cut?.cut || ''}`;
        const up = h('button', { class: 'btn', type: 'button', text: '↑', title: 'Move earlier' });
        const down = h('button', { class: 'btn', type: 'button', text: '↓', title: 'Move later' });
        up.disabled = index === 0; down.disabled = index === cut.clips.length - 1;
        up.addEventListener('click', () => edit([{ reorder_clip: { cut: cut.id, clip: clip.id, index: index - 1 } }], 'Move clip earlier'));
        down.addEventListener('click', () => edit([{ reorder_clip: { cut: cut.id, clip: clip.id, index: index + 1 } }], 'Move clip later'));
        const trim = (field, value) => {
          const input = h('input', { type: 'number', min: '0', step: '1', value: value ?? '', placeholder: field, 'aria-label': `${field} frame for ${label}` });
          input.dataset.preserveDraft = 'true';
          let editBase = state.version;
          input.addEventListener('focus', () => { editBase = state.version; });
          input.addEventListener('change', () => {
            const frame = input.value.trim() ? Number(input.value) : null;
            if (frame !== null && (!Number.isSafeInteger(frame) || frame < 0)) return;
            edit([{ update_clip: { cut: cut.id, clip: clip.id, patch: { [field]: frame ?? (field === 'in' ? 0 : null) } } }], 'Trim clip', { base: editBase, strictBase: true, onFail: render });
          });
          return input;
        };
        const deleteClip = h('button', { class: 'btn', type: 'button', text: 'Remove' });
        deleteClip.addEventListener('click', () => edit([{ remove_clip: { cut: cut.id, clip: clip.id } }], 'Remove clip'));
        const handle = h('button', { class: 'cut-drag-handle', type: 'button',
          'aria-label': `Drag ${label} to reorder`, title: 'Drag to reorder; arrow keys move one place', text: '⋮⋮' });
        const row = h('li', { class: 'cut-clip' }, handle, h('span', { text: `${index + 1}. ${label}` }), up, down,
          h('label', null, 'In', trim('in', clip.in || 0)), h('label', null, 'Out', trim('out', clip.out)),
          h('label', null, 'Slot', trim('slot', clip.slot)), deleteClip);
        let drag = null;
        const clearDrag = () => {
          row.classList.remove('dragging');
          for (const item of list.children) item.classList.remove('drop-target');
          drag = null;
        };
        const targetAt = (x, y) => {
          const target = document.elementFromPoint(x, y)?.closest('.cut-clip');
          if (!target || target.parentElement !== list) return;
          drag.target = cutDropTarget([...list.children], index, [...list.children].indexOf(target));
          for (const item of list.children) item.classList.toggle('drop-target', item === target);
        };
        handle.addEventListener('pointerdown', (event) => {
          if (event.button !== 0) return;
          event.preventDefault();
          drag = { project: state.project, version: state.version, cut: cut.id, target: index };
          row.classList.add('dragging');
          handle.setPointerCapture(event.pointerId);
        });
        handle.addEventListener('pointermove', (event) => {
          if (!drag) return;
          const bounds = dialog.getBoundingClientRect();
          if (event.clientY > bounds.bottom - 30) dialog.scrollTop += 15;
          if (event.clientY < bounds.top + 30) dialog.scrollTop -= 15;
          targetAt(event.clientX, event.clientY);
        });
        handle.addEventListener('pointerup', (event) => {
          if (!drag) return;
          targetAt(event.clientX, event.clientY);
          const { project, version, target } = drag;
          clearDrag();
          if (project === state.project && version === state.version && target !== index) {
            edit([{ reorder_clip: { cut: cut.id, clip: clip.id, index: target } }], 'Reorder clip');
          }
        });
        handle.addEventListener('pointercancel', clearDrag);
        handle.addEventListener('keydown', (event) => {
          const offset = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0;
          if (!offset) return;
          event.preventDefault(); event.stopPropagation();
          const target = index + offset;
          if (target >= 0 && target < cut.clips.length) {
            edit([{ reorder_clip: { cut: cut.id, clip: clip.id, index: target } }], 'Reorder clip');
          }
        });
        list.append(row);
      }
      dialog.append(list);
    };
    button.addEventListener('click', () => { open = true; dialog.hidden = false; render(); });
    dialog.addEventListener('focusout', () => setTimeout(render, 0));
    ctx.store.on('doc', render);
    ctx.store.on('selection', render);
    ctx.store.on('projectClosed', () => { open = false; dialog.hidden = true; render(); });
    ctx.store.on('beforeProjectChange', () => { open = false; dialog.hidden = true; });
    render();
    return {};
  },
});
