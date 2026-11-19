import { getHistory } from '../core/api.js';
import { h } from '../core/dom.js';
import { historyShotId } from '../core/history-map.js';
import { closePopover, openPopover } from '../core/popover.js';
import { registerPanel } from '../core/registry.js';
import { eventAuthor, relTime } from '../core/util.js';

registerPanel({
  id: 'history', title: 'History', region: 'toolbar', order: 14, optional: true, dock: { defaultDock: 'left', shortcut: 'Mod+Alt+4' },
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'History', 'aria-haspopup': 'dialog' });
    button.addEventListener('click', () => {
      const list = h('div', { class: 'history-list', text: 'Loading history…', role: 'status' });
      const panel = h('section', { class: 'history-popover', role: 'dialog', 'aria-label': 'Project history' }, h('h2', { text: 'History' }), list);
      const rect = button.getBoundingClientRect();
      openPopover(panel, { x: rect.left, y: rect.bottom + 4, anchor: button });
      const project = ctx.store.state.project;
      setTimeout(async () => {
        try {
          const result = await getHistory(project);
          if (ctx.store.state.project !== project || !panel.isConnected) return;
          const records = [...(result.history || [])].sort((left, right) => Date.parse(right.at || 0) - Date.parse(left.at || 0));
          list.replaceChildren(...records.map((record) => historyRow(record, ctx)));
          if (!records.length) list.textContent = 'No commits yet.';
        } catch (error) { if (panel.isConnected) list.textContent = `Could not load history: ${error.message}`; }
      }, 0);
    });
    el.append(button);
    return {};
  },
});

function historyRow(record, ctx) {
  const author = eventAuthor(record.author);
  const shotId = historyShotId(record, ctx.store.state.doc);
  const row = h('button', { class: `history-row ${author.kind}`, type: 'button', disabled: !shotId },
    h('span', { class: 'history-summary', text: record.summary || 'Commit' }),
    h('span', { class: 'history-meta', text: `${author.kind === 'agent' ? 'Agent: ' : 'Human: '}${author.name} · ${relTime(record.at)}` }),
    h('span', { class: 'history-shot', text: shotId ? `Show ${shotId}` : 'No current shot' }));
  if (shotId) row.addEventListener('click', () => { ctx.selection.selectOnly('shot', shotId, { focus: true }); ctx.emit('reveal', shotId); closePopover(); });
  return row;
}
