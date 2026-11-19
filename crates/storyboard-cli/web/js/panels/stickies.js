// Sticky notes: threads in the inspector, the shot's stickies section, free stickies on the graph
// (UX 3.2), and the N and Esc keys.

import { commit } from '../core/api.js';
import { autoGrow, els, h, preserveFocus } from '../core/dom.js';
import { KIND_GLYPH, KIND_LABEL, KINDS, nextId, pinTarget, takeOf } from '../core/model.js';
import { registerCommand, registerInspectorSection, registerPanel } from '../core/registry.js';
import { clearSelection, emit, state, toggleShowResolved } from '../core/store.js';
import { nowIso, relTime, stickyAuthor } from '../core/util.js';
import { NODE_W } from './graph/layout.js';
import { toWorld } from './graph/viewport.js';
import { resetInspector } from './inspector/index.js';
import { insp } from './inspector/shared.js';

let draftSticky = null;       // a free sticky being written, not yet committed

export function stickyActivity(root) {
  const replies = state.model.replies.get(root.id) || [];
  return [root, ...replies].reduce((latest, s) => (s.created > latest ? s.created : latest), root.created || '');
}

/** Thread view: root, replies, Resolve, Unpin and a reply field. */
export function renderThread(root, groupLabel) {
  const replies = [...(state.model.replies.get(root.id) || [])].sort((a, b) => (a.created < b.created ? -1 : 1));
  const item = (sticky) => {
    const author = stickyAuthor(sticky.author);
    return h('div', { class: 't-item' },
      h('div', { class: 't-meta' },
        h('span', { class: `who ${author.kind}`, text: author.name }),
        h('span', { class: 'kind', text: KIND_GLYPH[sticky.kind] || KIND_GLYPH.note, title: KIND_LABEL[sticky.kind] || 'Note' }),
        h('span', { dataset: { time: sticky.created || '' }, text: relTime(sticky.created) })),
      h('div', { class: 't-text', text: sticky.text }));
  };
  const thread = h('div', { class: `thread${root.resolved ? ' resolved' : ''}` });
  if (groupLabel) thread.append(h('div', { class: 'thread-group', text: groupLabel }));
  thread.append(item(root), ...replies.map(item));
  const actions = h('div', { class: 't-actions' });
  if (!root.resolved) {
    actions.append(h('button', { class: 'link-button', type: 'button', text: 'Resolve', onclick: () => commit([{ resolve_sticky: { sticky: root.id } }], { what: 'Resolve' }) }));
  } else actions.append(h('span', { class: 'preview-warn', style: { margin: 0 }, text: 'Resolved' }));
  const target = pinTarget(root.pin);
  if (target.shot) {
    actions.append(h('button', { class: 'link-button', type: 'button', text: 'Unpin', title: 'Make it a free note on the graph', onclick: () => unpinSticky(root, target.shot) }));
  }
  thread.append(actions);
  if (!root.resolved) {
    const reply = h('textarea', { class: 'reply-input', rows: '1', placeholder: 'Reply', 'aria-label': `Reply to ${stickyAuthor(root.author).name}`, dataset: { key: `reply:${root.id}` } });
    reply.value = insp.replyDrafts.get(root.id) || '';
    reply.addEventListener('input', () => { insp.replyDrafts.set(root.id, reply.value); autoGrow(reply); });
    reply.addEventListener('keydown', (event) => {
      event.stopPropagation();
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        const text = reply.value.trim();
        if (!text) return;
        commit([{ reply_sticky: { sticky: root.id, text, kind: 'note', author: 'human', created: nowIso() } }], {
          what: 'Reply', onDone: () => { insp.replyDrafts.delete(root.id); },
        });
        insp.replyDrafts.delete(root.id);
        reply.value = '';
      } else if (event.key === 'Escape') { reply.blur(); }
    });
    thread.append(reply);
  }
  return thread;
}

export function unpinSticky(sticky, shotId) {
  const p = state.layout.shots.get(shotId);
  const position = p ? { x: Math.round(p.x + NODE_W + 40), y: Math.round(p.y) } : { x: 0, y: 0 };
  commit([
    { update_sticky: { sticky: sticky.id, patch: { pin: 'none' } } },
    { move_node: { node: `sticky:${sticky.id}`, position } },
  ], { what: 'Unpin', label: `unpinned ${sticky.id}` });
}

export function patchShotStickies(entry, view) {
  const box = insp.parts.stickies;
  const shot = entry.shot;
  const roots = (state.model.pinnedToShot.get(shot.id) || []);
  const shown = roots.filter((s) => state.showResolved || !s.resolved).sort((a, b) => (stickyActivity(a) > stickyActivity(b) ? -1 : 1));
  const replies = shown.map((s) => (state.model.replies.get(s.id) || []).map((r) => [r.id, r.text]));
  const panelIndex = view.take && view.panel ? view.take.panels.indexOf(view.panel) + 1 : 0;
  const sig = JSON.stringify([shown, replies, state.showResolved, view.take?.id, view.take?.name, view.panel?.id, panelIndex, roots.length]);
  if (insp.sigs.stickies === sig) return;
  insp.sigs.stickies = sig;
  preserveFocus(box, () => {
    box.replaceChildren();
    const resolvedCount = roots.filter((s) => s.resolved).length;
    box.append(h('div', { class: 'section-head' }, 'Stickies', h('span', { class: 'count', text: String(roots.filter((s) => !s.resolved).length) })));
    box.append(newStickyForm(shot, view, panelIndex));
    if (!shown.length) box.append(h('div', { class: 'preview-warn', style: { margin: '4px 0' }, text: roots.length ? 'All notes here are resolved.' : 'No notes on this shot yet.' }));
    for (const root of shown) {
      const target = pinTarget(root.pin);
      let group = null;
      if (target.kind === 'take') group = `Take ${takeOf(shot, target.take)?.name || target.take}`;
      else if (target.kind === 'panel' || target.kind === 'layer') {
        const take = takeOf(shot, target.take);
        const index = take ? take.panels.findIndex((p) => p.id === target.panel) + 1 : 0;
        group = `${take && shot.takes.length > 1 ? `${take.name}, ` : ''}Panel ${index || target.panel}${target.kind === 'layer' ? `, layer ${target.layer}` : ''}`;
      }
      box.append(renderThread(root, group));
    }
    if (resolvedCount) {
      box.append(h('button', {
        class: 'link-button', type: 'button', style: { marginTop: '8px' },
        text: state.showResolved ? 'Hide resolved' : `Show resolved (${resolvedCount})`,
        onclick: () => { insp.sigs.stickies = ''; toggleShowResolved(); },
      }));
    }
  });
}

export function newStickyForm(shot, view, panelIndex) {
  const draft = insp.newSticky;
  const text = h('textarea', { rows: '2', placeholder: 'New note on this shot', 'aria-label': 'New sticky text', dataset: { key: `new-sticky:${shot.id}` } });
  text.value = draft.text;
  const kind = h('select', { 'aria-label': 'Kind', dataset: { key: 'new-sticky-kind' } }, KINDS.map((k) => h('option', { value: k, text: KIND_LABEL[k] })));
  kind.value = draft.kind;
  const pins = [['shot', `Shot ${state.model.shots.get(shot.id).seq}`]];
  if (view.take) pins.push(['take', `Take ${view.take.name || ''}`.trim()]);
  if (view.panel) pins.push(['panel', `Panel ${panelIndex}`]);
  const pin = h('select', { 'aria-label': 'Pin to', dataset: { key: 'new-sticky-pin' } }, pins.map(([value, label]) => h('option', { value, text: label })));
  pin.value = pins.some(([value]) => value === draft.pin) ? draft.pin : 'shot';
  const add = h('button', { class: 'btn small primary', type: 'button', text: 'Add note' });
  const submit = () => {
    const value = text.value.trim();
    if (!value) { text.focus(); return; }
    const pinText = pin.value === 'panel' && view.panel ? `panel:${shot.id}/${view.take.id}/${view.panel.id}`
      : pin.value === 'take' && view.take ? `take:${shot.id}/${view.take.id}` : `shot:${shot.id}`;
    commit([{ add_sticky: { text: value, kind: kind.value, author: 'human', pin: pinText, created: nowIso() } }], { what: 'Add note' });
    draft.text = '';
    text.value = '';
  };
  text.addEventListener('input', () => { draft.text = text.value; autoGrow(text); });
  text.addEventListener('keydown', (event) => {
    event.stopPropagation();
    if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) { event.preventDefault(); submit(); }
    if (event.key === 'Escape') text.blur();
  });
  kind.addEventListener('change', () => { draft.kind = kind.value; });
  pin.addEventListener('change', () => { draft.pin = pin.value; });
  add.addEventListener('click', submit);
  return h('div', { class: 'new-sticky' }, text, h('div', { class: 'row' }, kind, h('span', { class: 'preview-warn', style: { margin: 0 }, text: 'on' }), pin, h('span', { style: { flex: '1' } }), add));
}

export function renderStickyInspector(id) {
  const sticky = state.model.stickies.get(id);
  const root = sticky.reply_to ? state.model.stickies.get(sticky.reply_to) || sticky : sticky;
  const key = `sticky:${root.id}`;
  const replies = (state.model.replies.get(root.id) || []).map((r) => [r.id, r.text]);
  const sig = JSON.stringify([root, replies]);
  if (insp.key === key && insp.sigs.thread === sig) return;
  if (insp.key !== key) resetInspector(key);
  insp.sigs.thread = sig;
  const author = stickyAuthor(root.author);
  const body = h('div', { class: 'section' },
    h('div', { class: 'section-head' }, `${KIND_LABEL[root.kind] || 'Note'} by `, h('span', { class: `sticky-author ${author.kind}`, text: author.name }), h('span', { class: 'grow' })),
    renderThread(root, pinTarget(root.pin).kind ? null : 'Free on the graph. Drag it onto a shot, a panel or a take to pin it.'));
  preserveFocus(els.inspectorRoot, () => els.inspectorRoot.replaceChildren(h('div', { class: 'insp-scroll' }, body)));
}

/** UX 3.2: N with nothing selected writes a free sticky at the pointer (or the view centre). */
export function startFreeSticky(at) {
  if (!state.doc) return;
  cancelFreeSticky();
  let where = at;
  if (!where) {
    const rect = els.graph.getBoundingClientRect();
    const p = state.pointer;
    const inside = p && p.x >= rect.left && p.x <= rect.right && p.y >= rect.top && p.y <= rect.bottom;
    where = inside ? toWorld(p.x, p.y) : toWorld(rect.left + rect.width / 2 - 100, rect.top + rect.height / 2 - 50);
  }
  const textarea = h('textarea', { 'aria-label': 'New sticky text', placeholder: 'Write a note' });
  const kind = h('select', { 'aria-label': 'Kind' }, KINDS.map((k) => h('option', { value: k, text: KIND_LABEL[k] })));
  kind.style.cssText = 'background:transparent;border:0;color:var(--text-dim);font-size:11px';
  const el = h('div', { class: 'sticky-card draft', style: { transform: `translate(${Math.round(where.x)}px, ${Math.round(where.y)}px)` } },
    textarea, h('div', { class: 'draft-help' }, kind, ' Enter adds it, Esc cancels'));
  els.stickiesLayer.append(el);
  draftSticky = { el, where };
  textarea.focus({ preventScroll: true });
  const submit = () => {
    const text = textarea.value.trim();
    if (!text) { cancelFreeSticky(); return; }
    const id = nextId('sticky', state.model.stickies.keys());
    const position = { x: Math.round(where.x), y: Math.round(where.y) };
    cancelFreeSticky();
    commit([
      { add_sticky: { id, text, kind: kind.value, author: 'human', pin: 'none', created: nowIso() } },
      { move_node: { node: `sticky:${id}`, position } },
    ], { what: 'Add note', label: `added note ${id}` });
  };
  textarea.addEventListener('keydown', (event) => {
    event.stopPropagation();
    if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); submit(); }
    else if (event.key === 'Escape') { event.preventDefault(); cancelFreeSticky(); els.graph.focus({ preventScroll: true }); }
  });
  textarea.addEventListener('blur', () => setTimeout(() => {
    if (draftSticky?.el === el && !el.contains(document.activeElement)) {
      if (textarea.value.trim()) submit(); else cancelFreeSticky();
    }
  }, 0));
}
/** Removes the free sticky being written, if any. Returns whether there was one. */
export function cancelFreeSticky() {
  if (!draftSticky) return false;
  draftSticky.el.remove();
  draftSticky = null;
  return true;
}

export function newSticky() {
  const primary = state.primary;
  if (primary?.type === 'shot' && state.model.shots.has(primary.id)) {
    emit('revealInspector');
    const text = els.inspector.querySelector(`[data-key="new-sticky:${CSS.escape(primary.id)}"]`);
    if (text) { text.scrollIntoView({ block: 'nearest' }); text.focus({ preventScroll: true }); }
  } else startFreeSticky(null);
}

registerInspectorSection({
  id: 'stickies', title: 'Stickies', order: 70,
  mount(el) {
    const stickies = h('div', { class: 'section' });
    el.append(stickies);
    insp.parts.stickies = stickies;
    return { update(entry, view) { patchShotStickies(entry, view); } };
  },
});

registerPanel({
  id: 'stickies', title: 'Stickies', region: 'graph', order: 15, dock: { defaultDock: 'left', shortcut: 'Mod+Alt+3' },
  mount(el, ctx) {
    el.append(h('button', { class: 'btn small', type: 'button', text: 'New sticky', onclick: () => newSticky() }));
    ctx.store.on('beforeProjectChange', () => cancelFreeSticky());
    return {};
  },
});

registerCommand({
  id: 'sticky.new', title: 'New sticky', keys: ['n', 'N'], run: () => newSticky(),
  keymap: { order: 100, keys: 'N', label: 'New sticky (pinned to the selected shot)' },
});
registerCommand({
  id: 'selection.clear', title: 'Cancel or clear the selection', keys: ['Escape'],
  run: () => { if (!cancelFreeSticky()) clearSelection(); },
  keymap: { order: 200, keys: 'Esc', label: 'Cancel a drag or edit, close, then clear the selection' },
});
