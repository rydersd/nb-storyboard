// Toasts: server errors with their hint and valid values, and the conflict bar for writes that lost a race (UX 5).

import { commit } from '../core/api.js';
import { els, h, ICON_CLOSE, iconButton } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { notify, state } from '../core/store.js';
import { eventAuthor } from '../core/util.js';

export function toast(build, { kind = '', timeout = 7000 } = {}) {
  const el = h('div', { class: `toast ${kind}`, role: kind === 'conflict' ? 'alert' : 'status' });
  const close = iconButton('Dismiss', ICON_CLOSE, () => el.remove());
  build(el, close);
  els.toasts.append(el);
  while (els.toasts.children.length > 4) els.toasts.firstElementChild.remove();
  if (timeout) setTimeout(() => el.remove(), timeout);
  return el;
}

/** Errors show the server's message, hint and valid values, without blocking anything. */
export function showError(error, what) {
  const message = `${what ? `${what}: ` : ''}${error.message}`;
  notify.status({ text: `${message}${error.hint ? `. ${error.hint}` : ''}`, error: true });
  toast((el, close) => {
    el.append(h('div', { class: 't-head' }, h('div', { class: 't-msg', text: message }), close));
    if (error.hint) el.append(h('div', { class: 't-hint', text: error.hint }));
    if (error.valid?.length) el.append(h('div', { class: 't-valid' }, 'Valid: ', error.valid.map((value) => h('code', { text: String(value) }))));
    const issues = (error.issues || []).filter((issue) => issue.field && issue.message && issue.message !== error.message);
    if (issues.length) el.append(h('ul', null, issues.slice(0, 6).map((issue) => h('li', { text: `${issue.field}: ${issue.message}` }))));
  });
}

/** UX 5 conflict bar for non-field writes: who changed it, what landed, re-apply or discard. */
export function showConflict(error, others, ops, options) {
  const list = others.length ? others : error.intervening;
  const who = [...new Set(list.map((r) => eventAuthor(r.author).name))].join(', ') || 'Someone';
  notify.status({ text: `${who} changed this first. Showing the current state.`, error: true });
  toast((el, close) => {
    el.append(h('div', { class: 't-head' }, h('div', { class: 't-msg' }, h('strong', { text: who }), ' changed this first. The graph shows the current state.'), close));
    if (list.length) el.append(h('ul', null, list.slice(0, 6).map((r) => h('li', { text: `${eventAuthor(r.author).name}: ${r.summary}` }))));
    else el.append(h('div', { class: 't-hint', text: error.message }));
    if (ops) {
      el.append(h('div', { class: 'row' },
        h('button', { class: 'btn small primary', type: 'button', text: 'Re-apply on the current version', onclick: () => { el.remove(); commit(ops, { ...options, base: null }); } }),
        h('button', { class: 'btn small', type: 'button', text: 'Discard mine', onclick: () => el.remove() })));
    }
  }, { kind: 'conflict', timeout: 0 });
}

registerPanel({
  id: 'toasts', title: 'Messages', region: 'overlay', order: 10,
  mount(el, ctx) {
    els.toasts = h('div', { id: 'toasts', class: 'toasts', 'aria-live': 'polite' });
    el.append(els.toasts);
    ctx.store.on('error', ({ error, what }) => showError(error, what));
    ctx.store.on('conflict', ({ error, others, ops, options }) => showConflict(error, others, ops, options));
    return {};
  },
});
