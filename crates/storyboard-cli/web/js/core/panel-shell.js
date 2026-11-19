// Docked workbench panels. This is intentionally a small DOM adapter around the pure layout
// module: no panel state lives here, and pointer moves are coalesced to one rAF write.
import { getLayout, saveLayout } from './api.js';
import { h } from './dom.js';
import { getPanels, registerCommand } from './registry.js';
import { dropDock, defaultDockSize, layoutFor, movePanel, normalizeLayout, resizeDock } from './panel-layout.js';
import { emit, state } from './store.js';

let shell;
let current;
let persistTimer = 0;
const entries = new Map();

const dockPanels = () => getPanels().filter((panel) => panel.dock);
const dockIds = () => dockPanels().map((panel) => panel.id);

function defaultLayout() {
  let layout = layoutFor(dockIds());
  for (const panel of dockPanels()) {
    const dock = panel.dock?.defaultDock || 'right';
    if (dock !== 'right') layout = movePanel(layout, panel.id, dock);
  }
  return layout;
}

function container(dock) { return shell.querySelector(`[data-dock="${dock}"]`); }

function setPanelVisible(id, open) {
  const entry = entries.get(id);
  if (!entry) return;
  entry.root.hidden = !open;
  entry.slot.hidden = !open;
  entry.root.dataset.panelHidden = String(!open);
  emit('panelVisibility', { id, open });
  if (open) emit('panelOpened', id);
}

function apply(layout) {
  current = normalizeLayout(layout, dockIds());
  for (const dock of ['left', 'right', 'bottom']) {
    const target = container(dock);
    const panels = current[dock];
    for (const id of panels) {
      const entry = entries.get(id);
      if (entry && !entry.fixed) target.append(entry.root);
    }
    shell.style.setProperty(`--dock-${dock}`, panels.length ? `${current.sizes[dock] || defaultDockSize(dock)}px` : '0px');
  }
  for (const id of dockIds()) {
    const dock = ['left', 'right', 'bottom', 'center', 'hidden'].find((name) => current[name].includes(id));
    const entry = entries.get(id);
    if (entry && dock !== 'hidden') {
      const target = dock === 'center' ? entry.home : container(dock);
      if (target) target.insertBefore(entry.root, dock === 'center' ? entry.homeNext : null);
    }
    setPanelVisible(id, dock !== 'hidden');
  }
}

function queueSave() {
  clearTimeout(persistTimer);
  persistTimer = setTimeout(async () => {
    if (!state.workspace) return;
    try { current = normalizeLayout((await saveLayout(current)).layout, dockIds()); }
    catch (error) { console.warn('[workbench] could not save panel layout:', error); }
  }, 180);
}

function move(id, dock) {
  current = movePanel(current, id, dock);
  apply(current);
  queueSave();
}

function resize(dock, delta) {
  current = { ...current, sizes: resizeDock(current.sizes, dock, delta) };
  apply(current);
  queueSave();
}

function makeResizable(dock) {
  const target = container(dock);
  const separator = h('button', {
    class: 'dock-resizer', type: 'button', role: 'separator', tabindex: '0',
    'aria-orientation': dock === 'bottom' ? 'horizontal' : 'vertical',
    'aria-label': `Resize ${dock} dock`,
  });
  let pointer = null;
  let frame = 0;
  const flush = () => {
    frame = 0;
    if (!pointer) return;
    const delta = dock === 'bottom' ? pointer.startY - pointer.y : pointer.x - pointer.startX;
    const signed = dock === 'right' ? -delta : delta;
    const sizes = resizeDock(current.sizes, dock, signed);
    shell.style.setProperty(`--dock-${dock}`, `${sizes[dock]}px`);
  };
  separator.addEventListener('pointerdown', (event) => {
    pointer = { id: event.pointerId, startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY };
    separator.setPointerCapture(event.pointerId);
    event.preventDefault();
  });
  separator.addEventListener('pointermove', (event) => {
    if (!pointer || event.pointerId !== pointer.id) return;
    pointer.x = event.clientX; pointer.y = event.clientY;
    if (!frame) frame = requestAnimationFrame(flush);
  });
  const finish = (event) => {
    if (!pointer || event.pointerId !== pointer.id) return;
    const delta = dock === 'bottom' ? pointer.startY - event.clientY : event.clientX - pointer.startX;
    const signed = dock === 'right' ? -delta : delta;
    pointer = null;
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    resize(dock, signed);
  };
  separator.addEventListener('pointerup', finish);
  separator.addEventListener('pointercancel', finish);
  separator.addEventListener('keydown', (event) => {
    const keys = dock === 'bottom' ? ['ArrowUp', 'ArrowDown'] : ['ArrowLeft', 'ArrowRight'];
    if (!keys.includes(event.key)) return;
    // Keys move the splitter the way the arrow points, like the pointer: the right dock's splitter is on its
    // left edge, so ArrowLeft grows that dock.
    const direction = (event.key === 'ArrowUp' || event.key === 'ArrowRight' ? 1 : -1) * (dock === 'right' ? -1 : 1);
    resize(dock, direction * (event.shiftKey ? 40 : 16));
    // The splitter owns its arrow keys; without this the graph's Arrow commands also step the selection.
    event.preventDefault();
    event.stopPropagation();
  });
  target.append(separator);
}

function makeDraggable(id, tab) {
  let drag = null;
  let frame = 0;
  const preview = shell.querySelector('.panel-drop-preview');
  const update = () => {
    frame = 0;
    if (!drag) return;
    const dock = dropDock(drag.rect, drag.x, drag.y);
    drag.dock = dock;
    preview.hidden = !dock;
    if (!dock) return;
    const width = dock === 'bottom' ? drag.rect.width : Math.min(240, drag.rect.width / 3);
    const height = dock === 'bottom' ? Math.min(180, drag.rect.height / 3) : drag.rect.height;
    const x = dock === 'right' ? drag.rect.right - width : drag.rect.left;
    const y = dock === 'bottom' ? drag.rect.bottom - height : drag.rect.top;
    preview.style.width = `${width}px`; preview.style.height = `${height}px`;
    preview.style.transform = `translate(${Math.round(x - drag.rect.left)}px, ${Math.round(y - drag.rect.top)}px)`;
  };
  tab.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, dock: null, rect: shell.getBoundingClientRect() };
    tab.setPointerCapture(event.pointerId);
    shell.classList.add('dragging-panel');
  });
  tab.addEventListener('pointermove', (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    drag.x = event.clientX; drag.y = event.clientY;
    if (!frame) frame = requestAnimationFrame(update);
  });
  const finish = (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    const dock = drag.dock;
    drag = null;
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    preview.hidden = true;
    shell.classList.remove('dragging-panel');
    if (dock) move(id, dock);
  };
  tab.addEventListener('pointerup', finish);
  tab.addEventListener('pointercancel', finish);
}

function addEntry(panel) {
  const slot = document.querySelector(`[data-panel="${CSS.escape(panel.id)}"]`);
  if (!slot) return;
  const native = panel.id === 'graph' || panel.id === 'inspector';
  const root = native ? slot.closest('[data-region]') : h('section', { class: 'dock-panel', dataset: { panelShell: panel.id } });
  const home = root.parentElement;
  const homeNext = root.nextElementSibling;
  const tab = h('div', { class: 'dock-tab', role: 'button', tabindex: '0', text: panel.title, 'aria-label': `Move ${panel.title} panel` });
  if (native) root.prepend(tab);
  else {
    const content = h('div', { class: 'dock-content' });
    root.append(tab, content);
    content.append(slot);
  }
  entries.set(panel.id, { root, slot, home, homeNext });
  makeDraggable(panel.id, tab);
  const shortcut = panel.dock?.shortcut;
  if (shortcut) registerCommand({
    id: `panel.toggle.${panel.id}`, title: `Show or hide ${panel.title}`, keys: [shortcut], needsDoc: false,
    run: () => move(panel.id, current.hidden.includes(panel.id) ? (panel.dock?.defaultDock || 'right') : 'hidden'),
  });
}

export function installPanelShell() {
  shell = document.querySelector('#panel-shell');
  if (!shell) return;
  for (const panel of dockPanels()) addEntry(panel);
  for (const dock of ['left', 'right', 'bottom']) makeResizable(dock);
  current = defaultLayout();
  apply(current);
  window.addEventListener('beforeunload', () => clearTimeout(persistTimer), { once: true });
}

export async function restorePanelShell() {
  if (!shell || !state.workspace) return;
  try {
    const saved = await getLayout();
    apply(saved.layout || defaultLayout());
  } catch (error) {
    console.warn('[workbench] could not restore panel layout:', error);
    apply(defaultLayout());
  }
}

export function resetPanelShell() {
  current = defaultLayout();
  apply(current);
  queueSave();
}

export function togglePanelShell(id) {
  const panel = dockPanels().find((candidate) => candidate.id === id);
  if (!panel) return;
  move(id, current.hidden.includes(id) ? (panel.dock?.defaultDock || 'right') : 'hidden');
}
