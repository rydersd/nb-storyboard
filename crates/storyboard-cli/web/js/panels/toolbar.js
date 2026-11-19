// The toolbar's + Shot and + Note buttons; they run the same commands as A and N.

import { $, els, svg } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { state } from '../core/store.js';

export function renderAddButtons() {
  els.addShot.hidden = !state.doc;
  els.addNote.hidden = !state.doc;
}

registerPanel({
  id: 'add-buttons', title: 'Add', region: 'toolbar', order: 30,
  mount(el, ctx) {
    el.innerHTML = `<div class="tb-spacer"></div>
    <button id="add-shot-button" class="tb-button" title="Add shot (A)" hidden>
      <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M6 2v8M2 6h8" stroke="currentColor" stroke-width="1.5"/></svg> Shot
    </button>
    <button id="add-note-button" class="tb-button" title="New sticky (N)" hidden>
      <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M6 2v8M2 6h8" stroke="currentColor" stroke-width="1.5"/></svg> Note
    </button>`;
    els.addShot = $('#add-shot-button', el);
    els.addNote = $('#add-note-button', el);
    els.addShot.addEventListener('click', () => ctx.commands.run('shot.add'));
    els.addNote.addEventListener('click', () => ctx.commands.run('sticky.new'));
    for (const event of ['doc', 'loading', 'workspace', 'projectClosed']) ctx.store.on(event, renderAddButtons);
    return {};
  },
});
