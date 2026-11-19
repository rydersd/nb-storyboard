// The status bar's left side: the last change and who made it (click to frame it), and the drag hint.

import { els, h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { state } from '../core/store.js';
import { relTime } from '../core/util.js';

let lastChange = null;  // {who, text, at, ids}

export function setStatus({ text, who = null, at = null, ids = null, error = false }) {
  const el = els.statusLeft;
  lastChange = who ? { who, text, at, ids } : null;
  el.classList.toggle('error', !!error);
  el.classList.toggle('clickable', !!(ids && ids.length));
  el.replaceChildren();
  if (who) {
    el.append(h('span', { class: `who ${who.kind}`, text: who.name }), `: ${text}`, h('span', { class: 'when', text: at ? `, ${relTime(at)}` : '' }));
  } else el.textContent = text;
  el.title = el.textContent;
}
export function setHint(text) { els.statusHint.textContent = text; }

registerPanel({
  id: 'status', title: 'Status', region: 'status', order: 10,
  mount(el, ctx) {
    els.statusLeft = h('button', { id: 'status-left', class: 'status-left', type: 'button' });
    els.statusHint = h('span', { id: 'status-hint', class: 'status-hint' });
    el.append(els.statusLeft, els.statusHint);
    els.statusLeft.addEventListener('click', () => {
      const ids = lastChange?.ids?.filter((id) => state.layout?.shots.has(id) || state.layout?.stickies.has(id));
      if (ids?.length) ctx.emit('frame', ids);
    });
    setInterval(() => {
      if (lastChange?.at) { const when = els.statusLeft.querySelector('.when'); if (when) when.textContent = `, ${relTime(lastChange.at)}`; }
    }, 5000);
    ctx.store.on('status', setStatus);
    ctx.store.on('hint', setHint);
    return {};
  },
});
