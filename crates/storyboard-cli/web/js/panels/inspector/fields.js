// Editable inspector fields: commit on blur or Enter, revert on Esc, never overwrite typing, and
// show an agent's competing write as a conflict (UX 5, AC7).

import { commit } from '../../core/api.js';
import { autoGrow, els, h } from '../../core/dom.js';
import { state } from '../../core/store.js';
import { debounce, eventAuthor } from '../../core/util.js';
import { insp } from './shared.js';

let focusedField = null;
let fieldSeq = 0;
/**
 * An inspector field. Commits on blur, Enter (single line) or Cmd/Ctrl+Enter (multi-line); Esc
 * reverts. The base version is the one the person started typing on, so an agent's write to the
 * same thing in the meantime comes back as a conflict instead of being overwritten (AC7).
 */
export function makeField(spec) {
  const id = `f${++fieldSeq}`;
  const el = spec.multiline
    ? h('textarea', { id, rows: '1', dataset: { key: spec.key }, spellcheck: 'true' })
    : h('input', { id, type: 'text', dataset: { key: spec.key }, inputmode: spec.number ? 'decimal' : null, spellcheck: spec.number ? 'false' : 'true' });
  if (spec.className) el.className = spec.className;
  if (spec.bare) el.setAttribute('aria-label', spec.label);
  const remote = h('div', { class: 'remote-dot', hidden: true });
  const error = h('div', { class: 'field-error', hidden: true, role: 'alert' });
  const conflict = h('div', { class: 'conflict-bar', hidden: true, role: 'alert' });
  const wrap = spec.bare ? h('div', { class: 'field', style: { flex: '1', minWidth: '0' } }, el, remote, error, conflict) : h('div', { class: 'field' }, el, remote, error, conflict);
  const rec = { ...spec, el, wrap, remote, error, conflict, start: null, base: null, lastSeen: null, dirty: false, cancel: false };
  const nudge = debounce(() => commitField(rec, true), 500);
  el.addEventListener('focus', () => {
    focusedField = rec;
    if (!rec.dirty) { rec.start = el.value; rec.base = state.version; }
  });
  el.addEventListener('blur', () => {
    if (focusedField === rec) focusedField = null;
    nudge.cancel();
    if (rec.cancel) { rec.cancel = false; return; }
    commitField(rec);
  });
  el.addEventListener('input', () => { if (spec.multiline) autoGrow(el); });
  el.addEventListener('keydown', (event) => {
    event.stopPropagation();
    if (event.key === 'Escape') {
      event.preventDefault();
      rec.cancel = true;
      rec.dirty = false;
      el.value = rec.get();
      clearFieldNotes(rec);
      updateFramesLabel(rec);
      if (spec.multiline) autoGrow(el);
      el.blur();
      els.graph.focus({ preventScroll: true });
    } else if (event.key === 'Enter' && (!spec.multiline || event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      commitField(rec);
    } else if (spec.number && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
      event.preventDefault();
      const step = event.shiftKey ? 1 : 0.1;
      const current = Number(el.value) || 0;
      const next = Math.max(0, Math.round((current + (event.key === 'ArrowUp' ? step : -step)) * 1000) / 1000);
      el.value = String(next);
      updateFramesLabel(rec);
      nudge();
    }
  });
  insp.fields.set(spec.key, rec);
  return rec;
}

export function updateFramesLabel(rec) {
  if (!rec.framesEl) return;
  const seconds = Number(rec.el.value);
  rec.framesEl.textContent = Number.isFinite(seconds) && seconds > 0 ? `${Math.round(seconds * state.doc.fps)}f @ ${state.doc.fps}` : '';
}
export function clearFieldNotes(rec) {
  rec.remote.hidden = true;
  rec.error.hidden = true;
  rec.conflict.hidden = true;
}

/** Brings a field up to the document; never replaces text the person is typing. */
export function syncField(rec) {
  const current = rec.get();
  const editing = document.activeElement === rec.el || rec.dirty;
  if (editing) {
    if (rec.lastSeen !== null && current !== rec.lastSeen && current !== rec.el.value) {
      rec.remote.hidden = false;
      rec.remote.textContent = `${state.remoteName || 'Someone'} changed this`;
    }
  } else if (rec.el.value !== current) {
    rec.el.value = current;
    if (rec.lastSeen !== null && state.remoteName) {
      rec.wrap.classList.remove('tint');
      void rec.wrap.offsetWidth;
      rec.wrap.classList.add('tint');
    }
  }
  rec.lastSeen = current;
  updateFramesLabel(rec);
  if (rec.multiline) requestAnimationFrame(() => autoGrow(rec.el));
}

export function commitField(rec, keepFocus = false) {
  const raw = rec.el.value;
  const current = rec.get();
  if (raw === current) {
    rec.dirty = false;
    rec.start = raw;
    rec.base = state.version;
    rec.error.hidden = true;
    return;
  }
  if (raw === rec.start && !rec.dirty) return;
  const result = rec.toOps(raw);
  if (result.error) { showFieldError(rec, { message: result.error }); rec.dirty = true; return; }
  const base = rec.base || state.version;
  const startValue = rec.start;
  rec.dirty = true;
  rec.start = raw;
  rec.base = null;
  const done = () => {
    rec.dirty = false;
    rec.error.hidden = true;
    rec.conflict.hidden = true;
    rec.remote.hidden = true;
    rec.base = state.version;
    if (keepFocus || document.activeElement === rec.el) rec.start = rec.el.value;
  };
  const fail = (error) => { showFieldError(rec, error); };
  commit(result.ops, {
    base, what: rec.label, onDone: done, onError: fail,
    onOffline: () => showFieldError(rec, { message: 'Not saved: storyboard is offline. Your text is kept and saves when it reconnects.' }),
    onConflict: (error, others) => {
      const now = rec.get();
      if (now === startValue) {
        commit(result.ops, { what: rec.label, onDone: done, onError: fail });
      } else {
        showFieldConflict(rec, raw, now, others.length ? others : error.intervening);
      }
    },
  });
}

export function showFieldError(rec, error) {
  rec.error.hidden = false;
  const valid = error.valid?.length ? ` Valid: ${error.valid.join(', ')}.` : '';
  rec.error.textContent = `${error.message}${error.hint ? `. ${error.hint}` : ''}.${valid}`.replace(/\.\./g, '.');
}

/** UX 5: "codex changed this first", the current value, Keep theirs or Use mine. */
export function showFieldConflict(rec, mine, theirs, intervening) {
  const who = [...new Set(intervening.map((r) => eventAuthor(r.author).name))].join(', ') || 'Someone';
  rec.dirty = false;
  rec.el.value = theirs;
  rec.lastSeen = theirs;
  if (rec.multiline) autoGrow(rec.el);
  updateFramesLabel(rec);
  rec.remote.hidden = true;
  rec.conflict.hidden = false;
  rec.conflict.replaceChildren(
    h('div', null, h('span', { class: 'who', text: who }), ' changed this first.'),
    intervening.length ? h('ul', null, intervening.slice(0, 5).map((r) => h('li', { text: `${eventAuthor(r.author).name}: ${r.summary}` }))) : null,
    h('div', { text: 'Your draft:' }),
    h('div', { class: 'cur', text: mine || '(empty)' }),
    h('div', { class: 'row' },
      h('button', { class: 'btn small', type: 'button', text: 'Keep theirs', onclick: () => { rec.conflict.hidden = true; rec.start = rec.get(); } }),
      h('button', {
        class: 'btn small primary', type: 'button', text: 'Use mine',
        onclick: () => {
          rec.conflict.hidden = true;
          rec.el.value = mine;
          rec.start = theirs;
          rec.base = state.version;
          rec.dirty = true;
          commitField(rec, true);
        },
      })),
  );
}

/** Commits the focused field before the selection or project changes under it. */
export function commitFocusedField() {
  const rec = focusedField;
  if (!rec) return;
  commitField(rec, true);
}
