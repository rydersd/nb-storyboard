// The connection pill (Live, Reconnecting, Offline) and the offline banner.

import { els, h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { state } from '../core/store.js';

const CONN_LABEL = { live: 'Live', reconnecting: 'Reconnecting', offline: 'Offline', connecting: 'Connecting' };

registerPanel({
  id: 'connection', title: 'Connection', region: 'toolbar', order: 40,
  mount(el, ctx) {
    els.conn = h('span', { id: 'conn-pill', class: 'conn-pill', role: 'status', 'aria-live': 'polite' });
    el.append(els.conn);
    const render = () => {
      els.conn.dataset.state = state.conn;
      els.conn.textContent = CONN_LABEL[state.conn];
    };
    render();
    ctx.store.on('conn', render);
    return {};
  },
});

registerPanel({
  id: 'offline-banner', title: 'Offline banner', region: 'banner', order: 10,
  mount(el, ctx) {
    els.banner = h('div', { id: 'offline-banner', class: 'offline-banner', role: 'alert', hidden: true, text: 'Lost connection to storyboard. Edits are paused.' });
    el.append(els.banner);
    ctx.store.on('conn', () => { els.banner.hidden = !state.offline; });
    return {};
  },
});
