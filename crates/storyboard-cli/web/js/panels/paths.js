// The path picker: view story order or a cut (UI only, UX 2.4), and choose which one plays.

import { ApiFailure, commit, getProject, refreshProject } from '../core/api.js';
import { $, els, svg } from '../core/dom.js';
import { openMenu } from '../core/popover.js';
import { registerPanel } from '../core/registry.js';
import { applyServer, emit, notify, refresh, setSequence, setViewCut, state } from '../core/store.js';
import { saveView } from './graph/viewport.js';

export function openPathMenu() {
  const doc = state.doc;
  if (!doc) return;
  const items = [{ label: 'Story order', on: !doc.active_cut, viewing: !state.viewCut, run: () => viewCut(null) }];
  for (const cut of doc.cuts) items.push({ label: cut.name || cut.id, on: doc.active_cut === cut.id, viewing: state.viewCut === cut.id, run: () => viewCut(cut.id) });
  items.push({ sep: true });
  const viewed = state.viewCut && doc.cuts.find((cut) => cut.id === state.viewCut);
  if (viewed && doc.active_cut !== viewed.id) {
    items.push({ label: `Make "${viewed.name}" the active cut`, run: () => commit([{ set_active_cut: { cut: viewed.id } }], { what: 'Active cut', label: `"${viewed.name}" now plays` }) });
  } else if (!viewed && doc.active_cut) {
    items.push({ label: 'Play story order', run: () => commit([{ set_active_cut: { cut: null } }], { what: 'Active cut', label: 'story order now plays' }) });
  } else {
    items.push({ label: viewed ? `"${viewed.name}" is the active cut` : 'Story order plays', run: () => {} });
  }
  const rect = els.pathButton.getBoundingClientRect();
  openMenu(items, { x: rect.left, y: rect.bottom + 4 }, els.pathButton);
}

/** Viewing a cut is UI state only: it never writes (UX 2.4). */
export async function viewCut(cutId) {
  const project = state.project;
  const epoch = state.applyEpoch;
  setViewCut(cutId);
  saveView();
  refresh();
  try {
    const result = await getProject(project, cutId);
    if (state.project !== project || state.viewCut !== cutId) return;
    if (state.applyEpoch !== epoch) { refreshProject(); return; }
    setSequence(result.sequence);
    if (result.version !== state.version) applyServer(result.document, result.version, { self: false });
    else emit('inspector');
  } catch (error) {
    if (state.project === project && state.viewCut === cutId && error instanceof ApiFailure) notify.error(error, 'Viewing the cut');
  }
}

export function renderPathButton() {
  const doc = state.doc;
  const has = !!doc;
  els.pathButton.hidden = !has || !doc.cuts.length;
  if (!has) return;
  const viewed = state.viewCut && doc.cuts.find((cut) => cut.id === state.viewCut);
  els.pathName.textContent = viewed ? viewed.name : 'Story order';
  els.pathName.style.color = viewed ? 'var(--cut)' : '';
}

registerPanel({
  id: 'paths', title: 'Path', region: 'toolbar', order: 20,
  mount(el, ctx) {
    el.innerHTML = `<button id="path-button" class="tb-button" aria-haspopup="menu" aria-expanded="false" title="Choose which path to view" hidden>
      <span class="tb-label">Path</span> <span id="path-name">Story order</span>
      <svg class="chev" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 4.5l3 3 3-3" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>
    </button>`;
    els.pathButton = $('#path-button', el);
    els.pathName = $('#path-name', el);
    els.pathButton.addEventListener('click', openPathMenu);
    for (const event of ['doc', 'loading', 'workspace', 'projectClosed']) ctx.store.on(event, renderPathButton);
    return {};
  },
});
