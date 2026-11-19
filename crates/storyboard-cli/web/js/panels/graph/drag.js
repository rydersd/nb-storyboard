// Pointer and keyboard interaction on the graph: select, marquee, pan, move, insert (UX 3.1), pin a
// sticky by dropping it (UX 3.2), the context menu and renaming a scene.

import { addScene, addShot, addShotToScene, removeSelection, tidySelection } from '../../core/actions.js';
import { commit, optimisticCommit } from '../../core/api.js';
import { addKeyFilter } from '../../core/commands.js';
import { els, h, isTextTarget } from '../../core/dom.js';
import { closePopover, openMenu } from '../../core/popover.js';
import { registerCommand } from '../../core/registry.js';
import { adoptDeferred, clearSelection, notify, refresh, selectOnly, setDragging, setPointer, setPrimary, setShotSelection, setView, state, toggleSelect } from '../../core/store.js';
import { clamp } from '../../core/util.js';
import { GHOST, HIT_PX, nearestOnPolyline, NODE_H, NODE_W, portIn, portOut, worldBounds } from './layout.js';
import { frameEls, nodeEls, placeFrame, redrawWiresFor, stickyEls, storyWires } from './render.js';
import { applyView, frameBox, toWorld } from './viewport.js';
import { viewCut } from '../paths.js';
import { isReviewOpen } from '../review.js';
import { startFreeSticky } from '../stickies.js';

let dragState = null;  // the drag in progress: {kind, items, target, ...}
let spaceHeld = false;

export function hitTarget(target) {
  if (!(target instanceof Element)) return { kind: 'canvas' };
  const ghost = target.closest('.ghost-slot');
  if (ghost) return { kind: 'ghost', id: ghost.dataset.ghost };
  const bead = target.closest('.bead[data-clip-index]');
  if (bead && state.viewCut) return { kind: 'bead', index: Number(bead.dataset.clipIndex) };
  const node = target.closest('.node');
  if (node) return { kind: 'shot', id: node.dataset.shot };
  const sticky = target.closest('.sticky-card');
  if (sticky) return { kind: 'sticky', id: sticky.dataset.sticky, draft: sticky.classList.contains('draft') };
  const header = target.closest('.frame-header');
  if (header) return { kind: 'scene', id: header.dataset.scene };
  const pill = target.closest('.cut-pill');
  if (pill) return { kind: 'pill', id: pill.dataset.cut };
  // The graph's own dock tab (S19) belongs to the panel shell, not to a graph gesture.
  if (target.closest('.graph-empty, .review, .frame-title-input, .dock-tab')) return { kind: 'ui' };
  return { kind: 'canvas' };
}

let gesture = null; // {type: 'pan'|'marquee'|'press', ...}
export function capture(pointerId) {
  try { els.graph.setPointerCapture(pointerId); } catch { /* the pointer is already gone */ }
}
let rafPending = false;

export function scheduleFrame() {
  if (rafPending) return;
  rafPending = true;
  requestAnimationFrame(() => {
    rafPending = false;
    if (!gesture || !state.pointer) return;
    const { x, y } = state.pointer;
    if (gesture.type === 'pan') {
      setView({ x: gesture.vx + (x - gesture.sx), y: gesture.vy + (y - gesture.sy) });
      applyView();
    } else if (gesture.type === 'marquee') updateMarquee(x, y);
    else if (gesture.type === 'drag') updateDrag(x, y, state.pointer.alt);
  });
}

export function finishGesture(event, cancelled) {
  const current = gesture;
  gesture = null;
  if (!current) return;
  try { els.graph.releasePointerCapture(event.pointerId); } catch { /* already released */ }
  if (current.type === 'pan') els.graph.classList.remove('panning');
  else if (current.type === 'marquee') { els.marquee.hidden = true; }
  else if (current.type === 'bead') finishBead(current, event, cancelled);
  else if (current.type === 'drag') { if (cancelled) cancelDrag(); else dropDrag(event.altKey); }
  else if (current.type === 'press' && !current.shift && current.hit.kind === 'shot' && state.sel.shots.size > 1) {
    selectOnly('shot', current.hit.id);
  }
}

function beadAt(x, y) {
  const bead = document.elementFromPoint(x, y)?.closest('.bead[data-clip-index]');
  return bead && els.graph.contains(bead) ? bead : null;
}

function updateBead(gesture, x, y) {
  for (const bead of els.graph.querySelectorAll('.bead.drop-target')) bead.classList.remove('drop-target');
  const bead = beadAt(x, y);
  const index = bead ? Number(bead.dataset.clipIndex) : NaN;
  gesture.target = Number.isSafeInteger(index) && index >= 0 ? index : null;
  if (gesture.target !== null && gesture.target !== gesture.index) bead.classList.add('drop-target');
  els.chip.hidden = false;
  els.chip.textContent = gesture.target !== null && gesture.target !== gesture.index
    ? `Move clip ${gesture.index + 1} to ${gesture.target + 1}` : `Drag clip ${gesture.index + 1} onto another number`;
  els.chip.style.transform = `translate(${x + 14}px, ${y + 16}px)`;
}

function finishBead(gesture, event, cancelled) {
  if (!cancelled) updateBead(gesture, event.clientX, event.clientY);
  els.chip.hidden = true;
  for (const bead of els.graph.querySelectorAll('.bead.drop-target')) bead.classList.remove('drop-target');
  if (cancelled || gesture.target === null || gesture.target === gesture.index) return;
  if (state.project !== gesture.project || state.version !== gesture.version || state.viewCut !== gesture.cut) {
    notify.status({ text: 'The cut changed during the drag. Try again.', error: true });
    return;
  }
  commit([{ reorder_clip: { cut: gesture.cut, clip: gesture.clip, index: gesture.target } }], { base: gesture.version, what: 'Reorder clip' });
}

let hoveredFrame = null;
export function hoverFrames(event) {
  if (!state.layout) return;
  const p = toWorld(event.clientX, event.clientY);
  let found = null;
  for (const [id, frame] of state.layout.frames) {
    if (p.x >= frame.x && p.x <= frame.x + frame.w && p.y >= frame.y && p.y <= frame.y + frame.h) { found = id; break; }
  }
  if (found === hoveredFrame) return;
  frameEls.get(hoveredFrame)?.el.classList.remove('hover');
  hoveredFrame = found;
  frameEls.get(found)?.el.classList.add('hover');
}

// ---------------------------------------------------------------- marquee

export function updateMarquee(x, y) {
  const rect = els.graph.getBoundingClientRect();
  const left = Math.min(x, gesture.sx), top = Math.min(y, gesture.sy);
  const width = Math.abs(x - gesture.sx), height = Math.abs(y - gesture.sy);
  Object.assign(els.marquee.style, { left: `${left - rect.left}px`, top: `${top - rect.top}px`, width: `${width}px`, height: `${height}px` });
  els.marquee.hidden = width < 3 && height < 3;
  const a = toWorld(left, top), b = toWorld(left + width, top + height);
  const picked = new Set(gesture.additive ? gesture.base : []);
  for (const [id, p] of state.layout.shots) {
    if (p.x < b.x && p.x + NODE_W > a.x && p.y < b.y && p.y + NODE_H > a.y) picked.add(id);
  }
  const first = [...picked][0];
  const primaryChanged = (state.primary?.id || null) !== (first || null) && !(state.primary && picked.has(state.primary.id));
  if (primaryChanged) setShotSelection(picked, first ? { type: 'shot', id: first } : null);
  else setShotSelection(picked);
}

// ---------------------------------------------------------------- drag: move and insert

export function startDrag(press) {
  const hit = press.hit;
  const layout = state.layout;
  const drag = { kind: hit.kind, base: state.version, sx: press.sx, sy: press.sy, items: [], frames: new Map(), target: null };
  if (hit.kind === 'shot') {
    for (const id of state.sel.shots.size && state.sel.shots.has(hit.id) ? state.sel.shots : [hit.id]) {
      const p = layout.shots.get(id);
      if (p) drag.items.push({ key: `shot:${id}`, id, x0: p.x, y0: p.y, stored: p.stored, el: nodeEls.get(id).el });
    }
    drag.lead = hit.id;
  } else if (hit.kind === 'sticky') {
    const p = layout.stickies.get(hit.id);
    if (!p) return;
    drag.items.push({ key: `sticky:${hit.id}`, id: hit.id, x0: p.x, y0: p.y, stored: p.stored, el: stickyEls.get(hit.id).el });
  } else if (hit.kind === 'scene') {
    const scene = state.model.scenes.get(hit.id)?.scene;
    if (!scene || !scene.shots.length) { dragState = null; return; }
    for (const shot of scene.shots) {
      const p = layout.shots.get(shot.id);
      drag.items.push({ key: `shot:${shot.id}`, id: shot.id, x0: p.x, y0: p.y, stored: p.stored, el: nodeEls.get(shot.id).el });
    }
    drag.scene = hit.id;
  }
  for (const [id, frame] of layout.frames) drag.frames.set(id, { ...frame });
  for (const item of drag.items) item.el.classList.add('dragging');
  if (hit.kind === 'sticky') item0(drag).el.style.pointerEvents = 'none';
  dragState = drag;
  setDragging(true);
  notify.hint(drag.kind === 'shot'
    ? (state.viewCut ? 'Drag a clip number to reorder the cut; dragging a shot only moves it.' : 'Drop on the line to reorder, Alt to only move')
    : drag.kind === 'sticky' ? 'Drop on a shot, panel or take to pin' : 'Moving the scene');
  els.chip.hidden = false;
}
export const item0 = (drag) => drag.items[0];

export function updateDrag(clientX, clientY, alt) {
  const drag = dragState;
  if (!drag) return;
  const z = state.view.z;
  const dx = (clientX - drag.sx) / z, dy = (clientY - drag.sy) / z;
  drag.dx = dx; drag.dy = dy;
  for (const item of drag.items) item.el.style.transform = `translate(${item.x0 + dx}px, ${item.y0 + dy}px)`;
  if (drag.kind === 'scene') {
    const rec = frameEls.get(drag.scene);
    if (rec) placeFrame(rec, drag.frames.get(drag.scene), dx, dy);
  }
  if (drag.kind !== 'sticky') {
    const moving = new Map(drag.items.map((item) => [item.id, item]));
    redrawWiresFor(moving.keys(), (id) => {
      const item = moving.get(id);
      return item ? { x: item.x0 + dx, y: item.y0 + dy } : state.layout.shots.get(id);
    });
  }
  let target = null;
  if (drag.kind === 'shot' && drag.items.length === 1 && !alt && !state.viewCut) target = insertTarget(drag, toWorld(clientX, clientY));
  if (drag.kind === 'sticky') target = pinDropTarget(clientX, clientY);
  drag.target = target;
  showDropMarker(target);
  const chip = els.chip;
  chip.textContent = target ? target.label : 'Move';
  chip.classList.toggle('insert', !!target);
  chip.style.transform = `translate(${clientX + 14}px, ${clientY + 16}px)`;
}

/** UX 3.1: within 16 screen px of the story line, or inside a different scene's frame. */
export function insertTarget(drag, p) {
  const id = drag.lead;
  const z = state.view.z;
  const model = state.model;
  const own = model.shots.get(id);
  let best = null;
  for (const wire of storyWires) {
    if (wire.from === id || wire.to === id) continue;
    const near = nearestOnPolyline(wire.sample, p);
    if (near.d * z > HIT_PX || (best && near.d >= best.d)) continue;
    best = { d: near.d, near, wire };
  }
  if (best) {
    const a = model.shots.get(best.wire.from), b = model.shots.get(best.wire.to);
    const { near } = best;
    let scene, gap, label;
    if (a.scene.id === b.scene.id) { scene = a.scene; gap = a.index + 1; label = `Insert after ${a.seq}`; }
    else if (near.at < 0.5) { scene = a.scene; gap = a.scene.shots.length; label = `End of ${a.scene.title || `scene ${a.sceneIndex + 1}`}`; }
    else { scene = b.scene; gap = 0; label = `Start of ${b.scene.title || `scene ${b.sceneIndex + 1}`}`; }
    return { scene: scene.id, gap, label, point: { x: near.x, y: near.y }, tangent: { x: near.tx, y: near.ty } };
  }
  for (const [sceneId, frame] of drag.frames) {
    if (sceneId === own.scene.id) continue;
    if (p.x < frame.x || p.x > frame.x + frame.w || p.y < frame.y || p.y > frame.y + frame.h) continue;
    const scene = model.scenes.get(sceneId).scene;
    const name = scene.title || `scene ${model.scenes.get(sceneId).index + 1}`;
    if (!scene.shots.length) {
      return { scene: sceneId, gap: 0, label: `Into ${name}`, point: { x: frame.ghost.x + GHOST / 2, y: frame.ghost.y + GHOST / 2 }, tangent: { x: 1, y: 0 }, frame: sceneId };
    }
    const gaps = [];
    scene.shots.forEach((shot, index) => {
      const pos = state.layout.shots.get(shot.id);
      const left = portIn(pos), right = portOut(pos);
      if (index === 0) gaps.push({ gap: 0, x: left.x - 20, y: left.y });
      const next = scene.shots[index + 1] && state.layout.shots.get(scene.shots[index + 1].id);
      if (next) { const nl = portIn(next); gaps.push({ gap: index + 1, x: (right.x + nl.x) / 2, y: (right.y + nl.y) / 2 }); }
      else gaps.push({ gap: index + 1, x: right.x + 20, y: right.y });
    });
    let pick = gaps[0];
    for (const candidate of gaps) if (Math.hypot(candidate.x - p.x, candidate.y - p.y) < Math.hypot(pick.x - p.x, pick.y - p.y)) pick = candidate;
    const label = pick.gap === 0 ? `Start of ${name}` : pick.gap === scene.shots.length ? `End of ${name}` : `Insert after ${model.shots.get(scene.shots[pick.gap - 1].id).seq}`;
    return { scene: sceneId, gap: pick.gap, label, point: { x: pick.x, y: pick.y }, tangent: { x: 1, y: 0 }, frame: sceneId };
  }
  return null;
}

export function showDropMarker(target) {
  for (const [id, rec] of frameEls) rec.el.classList.toggle('drop-target', !!target?.frame && target.frame === id);
  if (!target || !target.point) { els.dropMarker.hidden = true; return; }
  const z = state.view.z;
  const half = 12 / z, cap = 8 / z;
  const { point: p, tangent: t } = target;
  const nx = -t.y, ny = t.x;
  const a = { x: p.x - t.x * half, y: p.y - t.y * half }, b = { x: p.x + t.x * half, y: p.y + t.y * half };
  const set = (el, p1, p2, width) => {
    el.setAttribute('x1', p1.x); el.setAttribute('y1', p1.y); el.setAttribute('x2', p2.x); el.setAttribute('y2', p2.y);
    el.style.strokeWidth = `${width / z}px`;
  };
  set(els.dropGap, a, b, 8);
  set(els.dropCapA, { x: a.x - nx * cap, y: a.y - ny * cap }, { x: a.x + nx * cap, y: a.y + ny * cap }, 2);
  set(els.dropCapB, { x: b.x - nx * cap, y: b.y - ny * cap }, { x: b.x + nx * cap, y: b.y + ny * cap }, 2);
  els.dropMarker.hidden = false;
}

export function endDragVisuals(drag) {
  for (const item of drag.items) { item.el.classList.remove('dragging'); item.el.style.pointerEvents = ''; }
  els.chip.hidden = true;
  els.dropMarker.hidden = true;
  for (const rec of frameEls.values()) rec.el.classList.remove('drop-target');
  for (const el of document.querySelectorAll('.drop-hover')) el.classList.remove('drop-hover');
  notify.hint('');
  dragState = null;
  setDragging(false);
}

export function cancelDrag() {
  const drag = dragState;
  if (!drag) return;
  endDragVisuals(drag);
  adoptDeferred();
  refresh();
}

export function dropDrag(alt) {
  const drag = dragState;
  if (!drag) return;
  const target = drag.kind === 'shot' && drag.items.length === 1 && !alt && !state.viewCut ? drag.target : drag.kind === 'sticky' ? drag.target : null;
  endDragVisuals(drag);
  const announce = adoptDeferred();
  const dx = drag.dx || 0, dy = drag.dy || 0;
  const pos = (item) => ({ x: clamp(Math.round(item.x0 + dx), -100000, 100000), y: clamp(Math.round(item.y0 + dy), -100000, 100000) });
  const options = { base: drag.base, what: 'Move' };
  if (drag.kind === 'sticky' && target) {
    const item = item0(drag);
    optimisticCommit([{ update_sticky: { sticky: item.id, patch: { pin: target.pin } } }], { ...options, label: `pinned ${item.id} to ${target.label.replace(/^Pin to /, '')}` });
  } else if (drag.kind === 'shot' && target) {
    const item = item0(drag);
    const entry = state.model.shots.get(item.id);
    const ops = [];
    if (target.scene === entry.scene.id) {
      const index = target.gap > entry.index ? target.gap - 1 : target.gap;
      if (index !== entry.index) ops.push({ move_shot: { shot: item.id, index } });
    } else {
      ops.push({ move_shot_between_scenes: { shot: item.id, scene: target.scene, index: target.gap } });
    }
    if (ops.length && item.stored) ops.push({ move_node: { node: item.key, position: pos(item) } });
    if (ops.length) optimisticCommit(ops, { ...options, what: 'Reorder' });
    else refresh();
  } else if (dx || dy) {
    optimisticCommit(drag.items.map((item) => ({ move_node: { node: item.key, position: pos(item) } })), options);
  } else {
    refresh();
  }
  announce?.();
}

/** A free sticky dropped on a shot node, a panel thumbnail or a take tab pins to it (UX 3.2). */
export function pinDropTarget(clientX, clientY) {
  for (const el of document.querySelectorAll('.drop-hover')) el.classList.remove('drop-hover');
  const under = document.elementFromPoint(clientX, clientY);
  if (!under) return null;
  const pinEl = under.closest('[data-pin]');
  if (pinEl) { pinEl.classList.add('drop-hover'); return { pin: pinEl.dataset.pin, label: `Pin to ${pinEl.dataset.pinLabel}` }; }
  const node = under.closest('.node');
  if (node) {
    const entry = state.model.shots.get(node.dataset.shot);
    return entry ? { pin: `shot:${entry.shot.id}`, label: `Pin to ${entry.seq}` } : null;
  }
  return null;
}

// ---------------------------------------------------------------- scene rename

export function startRenameScene(sceneId) {
  const rec = frameEls.get(sceneId);
  const entry = state.model.scenes.get(sceneId);
  if (!rec || !entry) return;
  rec.renaming = true;
  const base = state.version;
  const input = h('input', { class: 'frame-title-input', value: entry.scene.title || '', 'aria-label': 'Scene title' });
  rec.title.replaceWith(input);
  input.focus({ preventScroll: true });
  input.select();
  let done = false;
  const finish = (save) => {
    if (done) return;
    done = true;
    rec.renaming = false;
    input.replaceWith(rec.title);
    rec.sig = '';
    const title = input.value.trim();
    if (save && title !== (entry.scene.title || '')) commit([{ update_scene: { scene: sceneId, patch: { title } } }], { base, what: 'Rename' });
    else refresh();
  };
  input.addEventListener('keydown', (event) => {
    event.stopPropagation();
    if (event.key === 'Enter') finish(true);
    else if (event.key === 'Escape') finish(false);
  });
  input.addEventListener('pointerdown', (event) => event.stopPropagation());
  input.addEventListener('blur', () => finish(true));
}

/** Pointer, click and focus listeners on the graph. Called by the graph panel's mount. */
export function installDrag() {
  els.graph.addEventListener('pointerdown', (event) => {
    if (!state.doc || isReviewOpen()) return;
    closePopover();
    const hit = hitTarget(event.target);
    if (hit.kind === 'ui' || hit.kind === 'ghost' || (hit.kind === 'sticky' && hit.draft)) return;
    if (event.button === 1 || (event.button === 0 && spaceHeld)) {
      event.preventDefault();
      gesture = { type: 'pan', sx: event.clientX, sy: event.clientY, vx: state.view.x, vy: state.view.y };
      els.graph.classList.add('panning');
      capture(event.pointerId);
      return;
    }
    if (event.button !== 0) return;
    if (hit.kind === 'bead') {
      const cut = state.doc.cuts.find((item) => item.id === state.viewCut);
      const clip = cut?.clips[hit.index];
      if (!clip) return;
      event.preventDefault();
      gesture = { type: 'bead', project: state.project, version: state.version, cut: cut.id,
        clip: clip.id, index: hit.index, target: null };
      capture(event.pointerId);
      return;
    }
    if (!isTextTarget(document.activeElement)) els.graph.focus({ preventScroll: true });
    if (hit.kind === 'shot') {
      if (event.shiftKey) toggleSelect('shot', hit.id);
      else if (!state.sel.shots.has(hit.id)) selectOnly('shot', hit.id);
      else if (state.primary?.id !== hit.id) setPrimary('shot', hit.id);
      nodeEls.get(hit.id)?.el.focus({ preventScroll: true });
    } else if (hit.kind === 'sticky') {
      if (event.shiftKey) toggleSelect('sticky', hit.id);
      else if (!state.sel.stickies.has(hit.id)) selectOnly('sticky', hit.id);
    } else if (hit.kind === 'scene') {
      selectOnly('scene', hit.id);
    } else if (hit.kind === 'pill') {
      return;
    } else {
      if (!event.shiftKey) clearSelection();
      gesture = { type: 'marquee', sx: event.clientX, sy: event.clientY, additive: event.shiftKey, base: new Set(state.sel.shots) };
      capture(event.pointerId);
      return;
    }
    gesture = { type: 'press', hit, sx: event.clientX, sy: event.clientY, shift: event.shiftKey, pointerId: event.pointerId };
    capture(event.pointerId);
  });

  els.graph.addEventListener('pointermove', (event) => {
    setPointer({ x: event.clientX, y: event.clientY, alt: event.altKey });
    if (!gesture) { hoverFrames(event); return; }
    if (gesture.type === 'bead') { updateBead(gesture, event.clientX, event.clientY); return; }
    if (gesture.type === 'press') {
      if (Math.hypot(event.clientX - gesture.sx, event.clientY - gesture.sy) < 4) return;
      if (state.offline) { gesture = null; notify.status({ text: 'Offline: edits are paused.', error: true }); return; }
      startDrag(gesture);
      gesture = { type: 'drag' };
    }
    scheduleFrame();
  });

  els.graph.addEventListener('pointerup', (event) => finishGesture(event, false));
  els.graph.addEventListener('pointercancel', (event) => finishGesture(event, true));

  // sticky drags can leave the graph for the inspector: follow the pointer on the whole window
  window.addEventListener('pointermove', (event) => {
    if (dragState?.kind === 'sticky' && !els.graph.contains(event.target)) {
      setPointer({ x: event.clientX, y: event.clientY, alt: event.altKey });
      scheduleFrame();
    }
  });

  els.graph.addEventListener('click', (event) => {
    const hit = hitTarget(event.target);
    if (hit.kind === 'ghost') addShotToScene(hit.id, null);
  });
  els.graph.addEventListener('dblclick', (event) => {
    const hit = hitTarget(event.target);
    if (hit.kind === 'shot') { selectOnly('shot', hit.id, { focus: true }); frameBox(worldBounds([hit.id]), 1.5); }
    else if (hit.kind === 'scene') startRenameScene(hit.id);
    else if (hit.kind === 'pill') viewCut(hit.id);
    else if (hit.kind === 'sticky' && !hit.draft) selectOnly('sticky', hit.id);
  });
  els.graph.addEventListener('scroll', () => { els.graph.scrollTop = 0; els.graph.scrollLeft = 0; });

  els.graph.addEventListener('contextmenu', (event) => {
    if (!state.doc) return;
    const hit = hitTarget(event.target);
    if (hit.kind === 'ui') return;
    event.preventDefault();
    const items = [];
    if (hit.kind === 'shot') {
      if (!state.sel.shots.has(hit.id)) selectOnly('shot', hit.id);
      items.push({ label: 'Add shot after', key: 'A', run: () => addShot() });
      items.push({ label: 'Tidy to auto-layout', key: 'Shift+T', run: () => tidySelection() });
      items.push({ sep: true });
      items.push({ label: state.sel.shots.size > 1 ? `Remove ${state.sel.shots.size} shots` : 'Remove shot', key: 'Del', danger: true, run: () => removeSelection() });
    } else if (hit.kind === 'scene') {
      selectOnly('scene', hit.id);
      items.push({ label: 'Add shot at the end', run: () => addShotToScene(hit.id, null) });
      items.push({ label: 'Add scene after', key: 'Shift+A', run: () => addScene() });
      items.push({ label: 'Rename scene', run: () => startRenameScene(hit.id) });
      items.push({ sep: true });
      items.push({ label: 'Remove scene', key: 'Del', danger: true, run: () => removeSelection() });
    } else if (hit.kind === 'sticky' && !hit.draft) {
      selectOnly('sticky', hit.id);
      items.push({ label: 'Remove sticky', key: 'Del', danger: true, run: () => removeSelection() });
    } else {
      const where = toWorld(event.clientX, event.clientY);
      items.push({ label: 'New note here', key: 'N', run: () => startFreeSticky(where) });
      items.push({ label: 'Add scene', key: 'Shift+A', run: () => addScene() });
      items.push({ label: 'Frame everything', key: 'F', run: () => frameBox(worldBounds(null)) });
    }
    openMenu(items, { x: event.clientX, y: event.clientY });
  });

  // keyboard focus on a node selects it (Tab through the graph)
  els.nodesLayer.addEventListener('focusin', (event) => {
    const node = event.target.closest?.('.node');
    if (node && !state.sel.shots.has(node.dataset.shot) && !gesture) selectOnly('shot', node.dataset.shot);
  });
  document.addEventListener('keyup', (event) => {
    if (event.key === ' ') { spaceHeld = false; els.graph.classList.remove('space'); }
    if (event.key === 'Alt' && dragState && state.pointer) { setPointer({ ...state.pointer, alt: false }); scheduleFrame(); }
  });
  window.addEventListener('blur', () => { spaceHeld = false; els.graph.classList.remove('space'); });
}

// keys during a drag: Esc cancels it, Alt switches to move-only; nothing else runs
addKeyFilter((event) => {
  if (!dragState) return false;
  if (event.key === 'Escape') { event.preventDefault(); gesture = null; cancelDrag(); }
  else if (event.key === 'Alt' && state.pointer) { setPointer({ ...state.pointer, alt: true }); scheduleFrame(); }
  return true;
}, { beforeText: true });

registerCommand({
  id: 'graph.pan-mode', title: 'Pan while Space is held', keys: ['Space'],
  run: (ctx, event) => { if (!event?.repeat) { spaceHeld = true; els.graph.classList.add('space'); } },
});
