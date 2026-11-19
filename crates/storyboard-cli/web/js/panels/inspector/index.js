// The inspector panel: project summary, the selected shot (built from registered sections) or the
// selected sticky thread; its resizer and collapse.

import { $, els, h } from '../../core/dom.js';
import { context, getInspectorSections, guard, registerCommand, registerPanel } from '../../core/registry.js';
import { selectOnly, shotView, state } from '../../core/store.js';
import { clamp, debounce, fmtDuration, prefs, relTime } from '../../core/util.js';
import { commitField, commitFocusedField } from './fields.js';
import { insp } from './shared.js';
import { renderStickyInspector } from '../stickies.js';

export function renderInspector() {
  if (!state.doc) { els.inspectorRoot.replaceChildren(); insp.key = null; return; }
  const primary = state.primary;
  if (primary?.type === 'shot' && state.model.shots.has(primary.id)) return renderShotInspector(state.model.shots.get(primary.id));
  if (primary?.type === 'sticky' && state.model.stickies.has(primary.id)) return renderStickyInspector(primary.id);
  return renderSummary();
}

export function resetInspector(key) {
  if (insp.previewObjectUrl) URL.revokeObjectURL(insp.previewObjectUrl);
  insp.key = key;
  insp.fields.clear();
  insp.parts = null;
  insp.sigs = {};
  insp.previewUrl = '';
  insp.previewObjectUrl = '';
  for (const section of insp.sections) {
    if (section.handle?.unmount) {
      guard(`Inspector section "${section.def.title}"`, section.slot, () => section.handle.unmount());
    }
  }
  insp.sections = [];
  els.inspectorRoot.replaceChildren();
}

export function renderSummary() {
  const doc = state.doc;
  const key = 'summary';
  const shots = state.model.order.length;
  const frames = state.model.order.reduce((sum, id) => sum + Math.round((state.model.shots.get(id).shot.duration || 0) * doc.fps), 0);
  const open = doc.stickies.filter((s) => !s.reply_to && !s.resolved).length;
  const active = doc.cuts.find((cut) => cut.id === doc.active_cut);
  const viewed = state.viewCut && doc.cuts.find((cut) => cut.id === state.viewCut);
  const sig = JSON.stringify([doc.title, doc.scenes.length, shots, frames, open, active?.name, viewed?.name, state.sequence?.frames, state.removedBy]);
  if (insp.key === key && insp.sigs.summary === sig) return;
  if (insp.key !== key) resetInspector(key);
  insp.sigs.summary = sig;
  const rows = [
    ['Scenes', String(doc.scenes.length)],
    ['Shots', String(shots)],
    ['Story length', `${fmtDuration(frames, doc.fps)} (${frames} f @ ${doc.fps})`],
    ['Open stickies', String(open)],
    ['Plays', active ? `Cut "${active.name}"` : 'Story order'],
  ];
  if (viewed && state.sequence?.cut === viewed.id) rows.push([`Cut "${viewed.name}"`, `${fmtDuration(state.sequence.frames, doc.fps)} (${state.sequence.frames} f)`]);
  els.inspectorRoot.replaceChildren(h('div', { class: 'insp-scroll' },
    state.removedBy ? h('div', { class: 'removed-note' }, 'The selected shot was removed by ', h('strong', { text: state.removedBy }), '.') : null,
    h('div', { class: 'summary' },
      h('h2', { text: doc.title || state.project }),
      h('dl', null, rows.map(([term, value]) => [h('dt', { text: term }), h('dd', { text: value })]))),
    doc.scenes.length ? h('p', { class: 'insp-empty', text: 'Select a shot to see its panels, takes and notes.' }) : null,
  ));
}

/** The shot view: each registered section mounts once per shot, then patches itself on every change. */
export function renderShotInspector(entry) {
  const shot = entry.shot;
  const key = `shot:${shot.id}`;
  if (insp.key !== key) {
    resetInspector(key);
    mountSections(shot.id);
  }
  const view = shotView(shot);
  for (const section of insp.sections) {
    if (section.handle?.update) guard(`Inspector section "${section.def.title}"`, section.slot, () => section.handle.update(entry, view));
  }
}

export function mountSections(shotId) {
  insp.parts = {};
  const pinned = h('div', { class: 'insp-pinned' });
  const scroll = h('div', { class: 'insp-scroll' });
  for (const def of getInspectorSections()) {
    const slot = h('div', { class: 'section-slot', dataset: { section: def.id } });
    (def.pinned ? pinned : scroll).append(slot);
    const handle = guard(`Inspector section "${def.title}"`, slot, () => def.mount(slot, { ...context(), shotId }));
    insp.sections.push({ def, slot, handle });
  }
  els.inspectorRoot.replaceChildren(pinned, scroll);
}

export function toggleInspector() {
  const collapsed = els.app.classList.toggle('inspector-collapsed');
  prefs.set('inspector-collapsed', collapsed);
  if (!collapsed) { insp.previewUrl = ''; renderInspector(); }
}

registerPanel({
  id: 'inspector', title: 'Inspector', region: 'inspector', order: 10, dock: { defaultDock: 'right', shortcut: 'Mod+Alt+2' },
  mount(el, ctx) {
    els.inspector = el.closest('[data-region]');
    els.inspectorRoot = el;
    els.resizer = $('#inspector-resizer');
    const width = prefs.get('inspector-width');
    if (Number.isFinite(width)) els.inspector.style.width = `${width}px`;
    if (prefs.get('inspector-collapsed')) els.app.classList.add('inspector-collapsed');
    els.resizer.addEventListener('pointerdown', (event) => {
      event.preventDefault();
      els.resizer.setPointerCapture(event.pointerId);
      els.resizer.classList.add('active');
      const move = (e) => {
        const width = clamp(window.innerWidth - e.clientX, 360, window.innerWidth * 0.5);
        els.inspector.style.width = `${Math.round(width)}px`;
      };
      const up = () => {
        els.resizer.classList.remove('active');
        els.resizer.removeEventListener('pointermove', move);
        els.resizer.removeEventListener('pointerup', up);
        prefs.set('inspector-width', els.inspector.offsetWidth);
        renderInspector();
      };
      els.resizer.addEventListener('pointermove', move);
      els.resizer.addEventListener('pointerup', up);
    });
    els.resizer.addEventListener('keydown', (event) => {
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
      event.preventDefault();
      event.stopPropagation();
      const width = clamp(els.inspector.offsetWidth + (event.key === 'ArrowLeft' ? 24 : -24), 360, window.innerWidth * 0.5);
      els.inspector.style.width = `${width}px`;
      prefs.set('inspector-width', width);
      renderInspector();
    });
    window.addEventListener('resize', debounce(() => { if (state.doc) renderInspector(); }, 200));
    setInterval(() => {
      for (const time of els.inspector.querySelectorAll('[data-time]')) time.textContent = relTime(time.dataset.time);
    }, 5000);
    ctx.store.on('doc', () => renderInspector());
    ctx.store.on('selection', (info) => { if (!info?.classesOnly) renderInspector(); });
    ctx.store.on('inspector', () => renderInspector());
    ctx.store.on('beforeSelection', () => commitFocusedField());
    ctx.store.on('beforeProjectChange', () => commitFocusedField());
    ctx.store.on('projectDrop', () => {
      insp.replyDrafts.clear();
      insp.newSticky = { text: '', kind: 'note', pin: 'shot' };
      resetInspector(null);
    });
    ctx.store.on('focusTitle', (shotId) => {
      const title = insp.fields.get(`${shotId}:title`);
      if (title) { title.el.focus({ preventScroll: true }); title.el.select(); }
    });
    ctx.store.on('revealInspector', () => {
      if (els.app.classList.contains('inspector-collapsed')) toggleInspector();
      renderInspector();
    });
    ctx.store.on('sources', () => { insp.previewUrl = ''; insp.sigs.strip = ''; });
    ctx.store.on('reconnected', () => {
      for (const rec of insp.fields.values()) if (rec.dirty) commitField(rec, document.activeElement === rec.el);
    });
    return {};
  },
});

registerCommand({
  id: 'inspector.toggle', title: 'Show or hide the inspector', keys: ['Mod+\\'], global: true, needsDoc: false,
  run: () => toggleInspector(),
  keymap: { order: 190, keys: 'Cmd/Ctrl+\\', label: 'Show or hide the inspector' },
});
registerCommand({
  id: 'inspector.edit-title', title: 'Edit the shot title', keys: ['Enter'],
  run: (ctx, event) => {
    if (state.primary?.type === 'shot' && state.model.shots.has(state.primary.id)) {
      const title = insp.fields.get(`${state.primary.id}:title`);
      if (title) { title.el.focus({ preventScroll: true }); title.el.select(); }
    } else if (event?.target.closest?.('.sticky-card')) selectOnly('sticky', event.target.closest('.sticky-card').dataset.sticky);
  },
  keymap: { order: 130, keys: 'Enter', label: 'Edit the title of the selected shot' },
});
