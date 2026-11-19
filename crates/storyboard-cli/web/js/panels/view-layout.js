// View menu for docked panels. The shell owns server-backed layout state; hidden slots suppress
// store callbacks in main.js, so a closed tool does not keep rerendering behind the person's back.
import { els, h } from '../core/dom.js';
import { openMenu } from '../core/popover.js';
import { getPanels, registerPanel } from '../core/registry.js';
import { resetPanelShell, togglePanelShell } from '../core/panel-shell.js';

const visible = (id) => !document.querySelector(`[data-panel="${CSS.escape(id)}"]`)?.hidden;

registerPanel({
  id: 'view-layout', title: 'View', region: 'toolbar', order: 40,
  mount(el, ctx) {
    const button = h('button', { class: 'tb-button', type: 'button', text: 'View', 'aria-haspopup': 'menu' });
    button.addEventListener('click', () => {
      const items = getPanels().filter((panel) => panel.dock).map((panel) => ({
        label: panel.title, on: visible(panel.id), run: () => togglePanelShell(panel.id),
      }));
      items.push({ sep: true }, { label: 'Reset panel layout', run: resetPanelShell });
      const rect = button.getBoundingClientRect(); openMenu(items, { x: rect.left, y: rect.bottom + 4 }, button);
    });
    el.append(button); return {};
  },
});
