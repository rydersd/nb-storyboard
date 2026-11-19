import { h } from '../core/dom.js';
import { filterShotIds } from '../core/search-filter.js';
import { registerPanel } from '../core/registry.js';
import { setShotFilter, state } from '../core/store.js';

registerPanel({
  id: 'search-filter', title: 'Shot search and filter', region: 'toolbar', order: 12, optional: true,
  mount(el, ctx) {
    const input = h('input', { class: 'workbench-search', type: 'search', placeholder: 'Search shots', 'aria-label': 'Search title, action, dialogue, or notes' });
    const status = h('select', { class: 'workbench-filter', 'aria-label': 'Filter by status' },
      h('option', { value: '', text: 'All status' }), h('option', { value: 'draft', text: 'Draft' }),
      h('option', { value: 'review', text: 'Review' }), h('option', { value: 'approved', text: 'Approved' }), h('option', { value: 'changes', text: 'Changes' }));
    const sticky = h('label', { class: 'workbench-sticky-filter' }, h('input', { type: 'checkbox' }), ' Open notes');
    const checkbox = sticky.querySelector('input');
    const apply = () => {
      if (!state.model) return;
      const active = input.value.trim() || status.value || checkbox.checked;
      setShotFilter(active ? filterShotIds(state.model, { query: input.value, status: status.value, unresolved: checkbox.checked }) : null);
    };
    input.addEventListener('input', apply);
    status.addEventListener('change', apply);
    checkbox.addEventListener('change', apply);
    el.append(input, status, sticky);
    ctx.store.on('doc', apply);
    ctx.store.on('projectClosed', () => { input.value = ''; status.value = ''; checkbox.checked = false; setShotFilter(null); });
    return {};
  },
});
