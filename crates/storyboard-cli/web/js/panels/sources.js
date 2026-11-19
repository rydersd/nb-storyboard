// Project source browser. References and board changes always go through public operations.
import { commit, getSources, uploadShotImage } from '../core/api.js';
import { h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { sourceImageLayerOp, sourceLabel, sourceRefOps, sourceUseFor } from '../core/source-browser.js';
import { notify, shotView, state } from '../core/store.js';

function targetForSelection() {
  if (state.primary?.type !== 'shot') return null;
  const entry = state.model?.shots.get(state.primary.id);
  if (!entry) return null;
  const take = shotView(entry.shot).take;
  return take ? { kind: 'take', shot: entry.shot.id, take: take.id } : { kind: 'shot', shot: entry.shot.id };
}

function selectedPanel() {
  if (state.primary?.type !== 'shot') return null;
  const entry = state.model?.shots.get(state.primary.id);
  if (!entry) return null;
  const view = shotView(entry.shot);
  return view.take && view.panel ? { shot: entry.shot, take: view.take, panel: view.panel } : null;
}

function image(source) { return /^image\b/i.test(source.kind || '') || /\.(png|jpe?g|webp|gif)$/i.test(source.path || ''); }

registerPanel({
  id: 'sources', title: 'Sources', region: 'toolbar', order: 15, optional: true, dock: { defaultDock: 'left', shortcut: 'Mod+Alt+5' },
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'Sources', title: 'Browse project sources' });
    const box = h('section', { class: 'source-browser', hidden: true, 'aria-label': 'Project sources' });
    let data = { sources: [], missing_refs: [] };
    const render = () => {
      const target = targetForSelection();
      const rows = data.sources.map((source) => {
        const use = sourceUseFor(source, target);
        const row = h('div', { class: 'source-row', draggable: image(source), dataset: { path: source.path } },
          h('strong', { text: source.path }), h('small', { text: `${sourceLabel(source)}${source.used_by?.length ? ` · ${source.used_by.length} use${source.used_by.length === 1 ? '' : 's'}` : ''}` }));
        row.addEventListener('dragstart', (event) => { event.dataTransfer.setData('application/x-storyboard-source', source.path); event.dataTransfer.effectAllowed = 'copy'; });
        if (target) {
          const attach = h('button', { class: 'btn small', type: 'button', text: use ? 'Detach' : 'Attach' });
          attach.addEventListener('click', () => commit(sourceRefOps(use || target, source.path, !use), { what: use ? 'Detach source' : 'Attach source' }));
          row.append(attach);
        }
        return row;
      });
      const missing = data.missing_refs?.map((item) => h('p', { class: 'source-missing', text: `Missing: ${item.path} (${item.target})` })) || [];
      box.replaceChildren(h('header', null, h('strong', { text: 'Sources' }), h('button', { class: 'btn small', type: 'button', text: 'Close', onclick: () => { box.hidden = true; } })), ...missing, ...(rows.length ? rows : [h('p', { text: 'No sources found.' })]));
    };
    const load = async () => {
      if (!state.project) return;
      try { data = await getSources(state.project); render(); } catch (error) { notify.error(error, 'Loading sources'); }
    };
    button.addEventListener('click', () => { box.hidden = !box.hidden; if (!box.hidden) load(); });
    const dropImage = async (event) => {
      const panel = selectedPanel();
      if (!panel || !state.project) return;
      event.preventDefault();
      let path = event.dataTransfer.getData('application/x-storyboard-source');
      const file = event.dataTransfer.files?.[0];
      if (file) {
        if (!file.type.startsWith('image/')) { notify.status({ text: 'Drop an image file to place it on this board.', error: true }); return; }
        const folder = crypto.randomUUID();
        try { path = (await uploadShotImage(state.project, folder, file)).path; } catch (error) { notify.error(error, 'Importing image'); return; }
      }
      if (!path) return;
      commit([sourceImageLayerOp(panel.shot.id, panel.take.id, panel.panel.id, path)], { what: 'Place source image' });
    };
    document.addEventListener('dragover', (event) => {
      const types = [...(event.dataTransfer?.types || [])];
      if (types.includes('Files') || types.includes('application/x-storyboard-source')) event.preventDefault();
    });
    document.addEventListener('drop', (event) => {
      if (event.target.closest?.('.preview')) dropImage(event);
      else if (event.target.closest?.('.node')) {
        const path = event.dataTransfer?.getData('application/x-storyboard-source');
        const shot = event.target.closest?.('.node')?.dataset.shot;
        if (path && shot) { event.preventDefault(); commit(sourceRefOps({ kind: 'shot', shot }, path, true), { what: 'Attach source' }); }
      }
    });
    ctx.store.on('sources', () => { if (!box.hidden) load(); });
    ctx.store.on('selection', () => { if (!box.hidden) render(); });
    ctx.store.on('doc', () => { if (!box.hidden) load(); });
    el.append(button, box);
    return {};
  },
});
