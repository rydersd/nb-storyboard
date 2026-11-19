// Pan and zoom of the graph: wheel, pinch, framing, the zoom readout, and restoring a project's view.

import { els, h } from '../../core/dom.js';
import { saveViewport } from '../../core/api.js';
import { registerCommand, registerPanel } from '../../core/registry.js';
import { setView, state } from '../../core/store.js';
import { clamp, debounce, prefs } from '../../core/util.js';
import { viewportPayload } from '../../core/viewport-cache.js';
import { NODE_H, NODE_W, worldBounds } from './layout.js';
import { isReviewOpen } from '../review.js';

export const ZOOM_MIN = 0.1;
export const ZOOM_MAX = 4;
let gestureScale = 1;
let notifyViewChange = () => {};

export const saveView = debounce(() => {
  if (!state.project) return;
  const project = state.project;
  saveViewport(project, viewportPayload(state.view, state.viewCut)).catch(() => {
    // Only a failed cache route falls back to browser-local state.
    prefs.set(`view:${project}`, { x: state.view.x, y: state.view.y, z: state.view.z, cut: state.viewCut });
  });
}, 1000);

export function applyView() {
  const { x, y, z } = state.view;
  els.world.style.transform = `translate(${x}px, ${y}px) scale(${z})`;
  els.world.classList.toggle('lod-1', z < 0.45);
  els.world.classList.toggle('lod-2', z < 0.2);
  els.graph.style.backgroundSize = `${24 * z}px ${24 * z}px`;
  els.graph.style.backgroundPosition = `${x}px ${y}px`;
  if (els.zoom) els.zoom.textContent = `${Math.round(z * 100)}%`;
  notifyViewChange();
  saveView();
}
export function zoomAt(clientX, clientY, factor) {
  const rect = els.graph.getBoundingClientRect();
  const sx = clientX - rect.left, sy = clientY - rect.top;
  const z = clamp(state.view.z * factor, ZOOM_MIN, ZOOM_MAX);
  const k = z / state.view.z;
  setView({ x: sx - (sx - state.view.x) * k, y: sy - (sy - state.view.y) * k, z });
  applyView();
}
export function frameBox(box, maxZoom = 1.25) {
  if (!box) return;
  const rect = els.graph.getBoundingClientRect();
  const margin = 48;
  const z = clamp(Math.min((rect.width - margin * 2) / Math.max(box.w, 1), (rect.height - margin * 2) / Math.max(box.h, 1)), ZOOM_MIN, maxZoom);
  setView({ z, x: (rect.width - box.w * z) / 2 - box.x * z, y: (rect.height - box.h * z) / 2 - box.y * z });
  applyView();
}
export function frameSelectionOrAll() {
  const ids = [...state.sel.shots, ...state.sel.stickies];
  frameBox(worldBounds(ids.length ? ids : null));
}
export const toWorld = (clientX, clientY) => {
  const rect = els.graph.getBoundingClientRect();
  return { x: (clientX - rect.left - state.view.x) / state.view.z, y: (clientY - rect.top - state.view.y) / state.view.z };
};
export function ensureVisible(shotId) {
  const p = state.layout.shots.get(shotId);
  if (!p) return;
  const rect = els.graph.getBoundingClientRect();
  const z = state.view.z;
  const sx = p.x * z + state.view.x, sy = p.y * z + state.view.y;
  const margin = 40;
  const next = { x: state.view.x, y: state.view.y };
  if (sx < margin) next.x += margin - sx;
  else if (sx + NODE_W * z > rect.width - margin) next.x -= sx + NODE_W * z - (rect.width - margin);
  if (sy < margin) next.y += margin - sy;
  else if (sy + NODE_H * z > rect.height - margin) next.y -= sy + NODE_H * z - (rect.height - margin);
  setView(next);
  applyView();
}

/** Wheel and pinch on the graph, and the viewport's subscriptions. Called by the graph panel's mount. */
export function installViewport(ctx, onViewChanged = () => {}) {
  notifyViewChange = onViewChanged;
  els.graph.addEventListener('wheel', (event) => {
    if (!state.doc || isReviewOpen()) return;
    event.preventDefault();
    if (event.ctrlKey || event.metaKey) {
      const factor = Math.exp(-event.deltaY * (event.ctrlKey && !event.metaKey ? 0.01 : 0.002));
      zoomAt(event.clientX, event.clientY, factor);
      return;
    }
    // UX 3.3: deltaX or a fractional deltaY is a trackpad pan; whole-line or 120-step wheel deltas zoom
    const legacy = event.wheelDeltaY;
    const mouse = event.deltaMode === 1 || (event.deltaX === 0 && (typeof legacy === 'number' && legacy !== 0
      ? legacy % 120 === 0 : Number.isInteger(event.deltaY)));
    if (mouse && !event.shiftKey) zoomAt(event.clientX, event.clientY, Math.exp(-Math.sign(event.deltaY) * Math.min(Math.abs(event.deltaY), 120) * 0.0015));
    else {
      setView({
        x: state.view.x - (event.shiftKey && event.deltaX === 0 ? event.deltaY : event.deltaX),
        y: state.view.y - (event.shiftKey && event.deltaX === 0 ? 0 : event.deltaY),
      });
      applyView();
    }
  }, { passive: false });

  // Safari reports pinch as gesture events rather than ctrl+wheel
  els.graph.addEventListener('gesturestart', (event) => { event.preventDefault(); gestureScale = 1; });
  els.graph.addEventListener('gesturechange', (event) => {
    event.preventDefault();
    const p = state.pointer || { x: event.clientX, y: event.clientY };
    zoomAt(p.x, p.y, event.scale / gestureScale);
    gestureScale = event.scale;
  });
  ctx.store.on('projectOpened', ({ saved }) => {
    if (saved && Number.isFinite(saved.z)) { setView({ x: saved.x, y: saved.y, z: clamp(saved.z, ZOOM_MIN, ZOOM_MAX) }); applyView(); }
    else {
      const bounds = worldBounds(null);
      if (bounds) frameBox(bounds, 1);
      else { setView({ x: 40, y: 40, z: 1 }); applyView(); }
    }
  });
  ctx.store.on('frame', (ids) => { if (ids?.length) frameBox(worldBounds(ids), 1); });
  ctx.store.on('reveal', (shotId) => ensureVisible(shotId));
}

registerPanel({
  id: 'zoom', title: 'Zoom', region: 'status', order: 20,
  mount(el) {
    els.zoom = h('button', {
      id: 'status-zoom', class: 'status-zoom', type: 'button', text: '100%',
      title: 'Zoom to a shot-sized view', 'aria-label': 'Zoom to a shot-sized view',
      onclick: () => {
        const selected = state.primary?.type === 'shot' ? state.primary.id : null;
        const bounds = selected && worldBounds([selected]);
        if (bounds) frameBox(bounds, 1);
        else {
          const rect = els.graph.getBoundingClientRect();
          zoomAt(rect.left + rect.width / 2, rect.top + rect.height / 2, 1 / state.view.z);
        }
      },
    });
    el.append(els.zoom);
    return {};
  },
});

registerCommand({
  id: 'view.frame', title: 'Frame the selection, or everything', keys: ['f', 'F'], run: () => frameSelectionOrAll(),
  keymap: { order: 30, keys: 'F', label: 'Frame the selection, or everything' },
});
registerCommand({
  id: 'view.actual-size', title: 'Zoom to 100%', keys: ['0'],
  run: () => {
    const rect = els.graph.getBoundingClientRect();
    zoomAt(rect.left + rect.width / 2, rect.top + rect.height / 2, 1 / state.view.z);
  },
  keymap: { order: 40, keys: '0', label: 'Zoom to 100%' },
});
