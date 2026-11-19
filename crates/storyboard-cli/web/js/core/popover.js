// Popovers and menus: one at a time, over a scrim, closed by Escape or a click outside.

import { h } from './dom.js';
import { clamp } from './util.js';

let popover = null;
export function closePopover() {
  if (!popover) return;
  const { el, scrim, anchor, onClose } = popover;
  popover = null;
  el.remove();
  scrim?.remove();
  anchor?.setAttribute('aria-expanded', 'false');
  onClose?.();
}
export function openPopover(el, { x, y, anchor, onClose, center }) {
  closePopover();
  const scrim = h('div', { class: 'scrim' });
  scrim.addEventListener('pointerdown', (event) => { event.preventDefault(); closePopover(); });
  popoverRoot().append(scrim, el);
  el.classList.add('popover');
  if (center) {
    el.style.left = `${Math.max(8, (window.innerWidth - el.offsetWidth) / 2)}px`;
    el.style.top = `${Math.max(48, window.innerHeight * 0.12)}px`;
  } else {
    el.style.left = `${clamp(x, 8, window.innerWidth - el.offsetWidth - 8)}px`;
    el.style.top = `${clamp(y, 8, window.innerHeight - el.offsetHeight - 8)}px`;
  }
  anchor?.setAttribute('aria-expanded', 'true');
  popover = { el, scrim, anchor, onClose };
  return popover;
}

/** items: [{label, key?, run, danger?, on?, status?, sep?, viewing?}] */
export function openMenu(items, at, anchor) {
  const menu = h('div', { class: 'menu', role: 'menu' });
  for (const item of items) {
    if (item.sep) { menu.append(h('div', { class: 'sep', role: 'separator' })); continue; }
    const mark = item.status ? h('i', { class: 'mark status', style: { background: `var(--status-${item.status})` } })
      : item.on !== undefined ? h('i', { class: `mark${item.on ? ' on' : ''}` }) : null;
    const button = h('button', { type: 'button', role: 'menuitem', class: `${item.danger ? 'danger' : ''} ${item.viewing ? 'viewing' : ''}`.trim() || null },
      mark, item.label, item.key ? h('span', { class: 'key', text: item.key }) : null);
    button.addEventListener('click', () => { closePopover(); item.run(); });
    menu.append(button);
  }
  menu.addEventListener('keydown', (event) => {
    const buttons = [...menu.querySelectorAll('button')];
    const index = buttons.indexOf(document.activeElement);
    if (event.key === 'ArrowDown') { event.preventDefault(); buttons[(index + 1) % buttons.length]?.focus(); }
    else if (event.key === 'ArrowUp') { event.preventDefault(); buttons[(index - 1 + buttons.length) % buttons.length]?.focus(); }
    else if (event.key === 'Escape') { event.preventDefault(); closePopover(); anchor?.focus(); }
    event.stopPropagation();
  });
  openPopover(menu, { ...at, anchor });
  menu.querySelector('button')?.focus({ preventScroll: true });
}

function popoverRoot() {
  let root = document.getElementById('popover-root');
  if (!root) {
    root = h('div', { id: 'popover-root' });
    document.getElementById('app').append(root);
  }
  return root;
}
/** The open popover record ({el, anchor, ...} plus whatever its opener added), or null. */
export const currentPopover = () => popover;
export const isPopoverOpen = () => popover !== null;
