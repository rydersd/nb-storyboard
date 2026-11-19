// The graph panel: scene frames, shot nodes, canvas stickies, story line and cut path, patched
// in place from the store on every change; and the built-in node badges.

import { addScene } from '../../core/actions.js';
import { panelUrl } from '../../core/api.js';
import { $, els, h, ICON_COPY, ICON_PLUS, iconButton, svg } from '../../core/dom.js';
import { KIND_GLYPH, KIND_LABEL, shotStatus, STATUS_LABEL, takeOf } from '../../core/model.js';
import { getNodeBadges, guard, registerNodeBadge, registerPanel } from '../../core/registry.js';
import { notify, selectOnly, state } from '../../core/store.js';
import { debounce, fmtSeconds, stickyAuthor } from '../../core/util.js';
import { installDrag, startRenameScene } from './drag.js';
import { GHOST, NODE_H, NODE_THUMB_H, NODE_W, PILL_H, PILL_W, portIn, portOut, sampleSegments, segmentsPath, wireSegments, worldBounds } from './layout.js';
import { applyView, frameBox, installViewport } from './viewport.js';

const GRAPH_HTML = `
<div id="world" class="world">
  <div id="frames-layer" class="layer"></div>
  <svg id="wires" class="wires" width="1" height="1" aria-hidden="true">
    <defs>
      <marker id="arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
        <path d="M0 0.5L8 4L0 7.5z" class="arrow-head"/>
      </marker>
      <marker id="arrow-cut" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
        <path d="M0 0.5L8 4L0 7.5z" class="arrow-head-cut"/>
      </marker>
    </defs>
    <g id="story-wires"></g>
    <g id="cut-wires"></g>
  </svg>
  <div id="nodes-layer" class="layer"></div>
  <div id="stickies-layer" class="layer"></div>
  <svg class="wires" width="1" height="1" aria-hidden="true">
    <g id="drop-marker" class="drop-marker" hidden>
      <line id="drop-gap" class="drop-gap"/>
      <line id="drop-cap-a" class="drop-cap"/>
      <line id="drop-cap-b" class="drop-cap"/>
    </g>
  </svg>
</div>
<div id="marquee" class="marquee" hidden></div>
<div id="drag-chip" class="drag-chip" hidden></div>
<div id="graph-empty" class="graph-empty" hidden></div>`;

export const nodeEls = new Map();    // shot id -> record
export const frameEls = new Map();   // scene id -> record
export const stickyEls = new Map();  // sticky id -> record
export const pillEls = new Map();    // clip key -> element
export let storyWires = [];          // [{from, to, segments, sample, el}]
export const wiresByShot = new Map();
const THUMB_WIDTHS = [240, 360, 480, 720, 960, 1440, 1920];
let previewWidth = 0;

export function thumbnailRenderWidth(zoom = state.view.z, pixelRatio = window.devicePixelRatio || 1) {
  const needed = NODE_W * zoom * pixelRatio;
  return THUMB_WIDTHS.find((width) => width >= needed) || 1920;
}

const refreshThumbnailResolution = debounce(() => {
  if (state.doc && state.project) renderNodes(!!state.viewCut);
}, 120);

function onViewChanged() {
  const nextWidth = thumbnailRenderWidth();
  if (previewWidth === nextWidth) return;
  previewWidth = nextWidth;
  refreshThumbnailResolution();
}

export function renderGraph() {
  const doc = state.doc;
  const viewingCut = !!state.viewCut;
  els.graph.classList.toggle('viewing-cut', viewingCut);
  renderGraphEmpty();
  renderFrames();
  previewWidth = thumbnailRenderWidth();
  renderNodes(viewingCut);
  renderStickies();
  renderStoryWires();
  renderCutPath();
  applyView();
  if (doc.scenes.length === 0) els.graph.classList.add('is-empty'); else els.graph.classList.remove('is-empty');
}

export function renderGraphEmpty() {
  const box = els.graphEmpty;
  box.replaceChildren();
  if (!state.project) {
    const hasProjects = !!state.workspace?.projects?.length;
    box.append(
      h('h2', { text: hasProjects ? 'No project open.' : 'No projects yet.' }),
      h('p', { text: hasProjects ? 'Open a saved project or create a new one.' : 'Create a project to start your storyboard.' }),
      h('button', { class: 'btn primary', type: 'button', text: 'Choose project', onclick: () => els.projectButton.click() }),
    );
    box.hidden = !state.workspace;
    return;
  }
  if (state.doc && state.doc.scenes.length === 0) {
    const line = `storyboard add-scene ${state.project} --base-version ${state.version} --title "Opening"`;
    box.append(
      h('h2', { text: 'No scenes yet.' }),
      h('button', { class: 'btn primary', type: 'button', text: 'Add scene', onclick: () => addScene() }),
      h('p', { text: 'Or ask the agent to add one:' }),
      cliLine(line),
    );
    box.hidden = false;
    return;
  }
  box.hidden = true;
}
export function cliLine(text) {
  const code = h('code', { text });
  const copy = iconButton('Copy command', ICON_COPY, async () => {
    try { await navigator.clipboard.writeText(text); notify.status({ text: 'Command copied.' }); }
    catch { notify.status({ text: 'Copy failed: select the command and copy it by hand.', error: true }); }
  });
  return h('div', { class: 'cli-line' }, code, copy);
}

export function renderFrames() {
  const { doc, layout, model } = state;
  const seen = new Set();
  doc.scenes.forEach((scene, index) => {
    seen.add(scene.id);
    let rec = frameEls.get(scene.id);
    if (!rec) {
      const num = h('span', { class: 'frame-num' });
      const title = h('span', { class: 'frame-title' });
      const chip = h('span', { class: 'frame-chip', hidden: true });
      const orderHandle = h('button', { class: 'scene-order-handle', type: 'button', text: '↔', title: 'Drag to reorder scene', 'aria-label': 'Drag to reorder scene', dataset: { reorderKind: 'scene', reorderId: scene.id } });
      orderHandle.addEventListener('click', (event) => { event.stopPropagation(); selectOnly('scene', scene.id); });
      const header = h('div', { class: 'frame-header', dataset: { scene: scene.id } }, orderHandle, num, title, chip);
      const ghost = h('button', { class: 'ghost-slot', type: 'button', html: ICON_PLUS, dataset: { ghost: scene.id } });
      const empty = h('div', { class: 'frame-empty', text: 'Empty scene' });
      const el = h('div', { class: 'frame', dataset: { scene: scene.id } }, header, empty, ghost);
      rec = { el, header, num, title, chip, ghost, empty, orderHandle, sig: '' };
      frameEls.set(scene.id, rec);
      els.framesLayer.append(el);
    }
    const frame = layout.frames.get(scene.id);
    const pinned = (model.pinnedToScene.get(scene.id) || []).filter((s) => !s.resolved).length;
    const sig = JSON.stringify([index, scene.title, pinned, frame.empty]);
    if (rec.sig !== sig) {
      rec.sig = sig;
      rec.num.textContent = String(index + 1);
      if (!rec.renaming) {
        rec.title.textContent = scene.title || 'Untitled scene';
        rec.title.classList.toggle('untitled', !scene.title);
      }
      rec.chip.hidden = pinned === 0;
      rec.chip.textContent = `${pinned} note${pinned === 1 ? '' : 's'}`;
      rec.empty.hidden = !frame.empty;
      rec.el.classList.toggle('empty', frame.empty);
      rec.ghost.setAttribute('aria-label', `Add shot at the end of scene ${index + 1} ${scene.title || ''}`.trim());
      rec.ghost.title = 'Add shot at the end of this scene';
    }
    placeFrame(rec, frame);
    rec.el.classList.toggle('selected', state.sel.scene === scene.id);
  });
  for (const [id, rec] of frameEls) if (!seen.has(id)) { rec.el.remove(); frameEls.delete(id); }
}
export function placeFrame(rec, frame, dx = 0, dy = 0) {
  rec.el.style.transform = `translate(${frame.x + dx}px, ${frame.y + dy}px)`;
  rec.el.style.width = `${frame.w}px`;
  rec.el.style.height = `${frame.h}px`;
  rec.ghost.style.transform = `translate(${frame.ghost.x - frame.x}px, ${frame.ghost.y - frame.y}px)`;
  rec.empty.style.top = `${frame.ghost.y - frame.y + 10}px`;
  rec.empty.style.left = `${frame.ghost.x - frame.x + GHOST + 12}px`;
}

export function renderNodes(viewingCut) {
  const { doc, layout, model } = state;
  const beads = new Map();
  const inCut = new Set();
  if (viewingCut) {
    const cut = doc.cuts.find((candidate) => candidate.id === state.viewCut);
    cut?.clips.forEach((clip, index) => {
      const shotId = clip.source?.shot?.shot;
      if (!shotId) return;
      inCut.add(shotId);
      if (!beads.has(shotId)) beads.set(shotId, []);
      beads.get(shotId).push(index + 1);
    });
  }
  const seen = new Set();
  for (const id of model.order) {
    seen.add(id);
    const entry = model.shots.get(id);
    let rec = nodeEls.get(id);
    if (!rec) {
      rec = createNode(id);
      nodeEls.set(id, rec);
      els.nodesLayer.append(rec.el);
    }
    patchNode(rec, entry, { state, entry, clips: beads.get(id) || [] });
    const p = layout.shots.get(id);
    rec.el.style.transform = `translate(${p.x}px, ${p.y}px)`;
    rec.el.classList.toggle('selected', state.sel.shots.has(id));
    rec.el.classList.toggle('dimmed', (viewingCut && !inCut.has(id)) || !!(state.shotFilter && !state.shotFilter.has(id)));
  }
  for (const [id, rec] of nodeEls) if (!seen.has(id)) { rec.el.remove(); nodeEls.delete(id); }
}

export function createNode(id) {
  const img = h('img', { alt: '', loading: 'lazy', decoding: 'async', draggable: 'false' });
  const bar = h('div', { class: 'bar' });
  const noPanel = h('div', { class: 'no-panel', text: 'No panels', hidden: true });
  const thumb = h('div', { class: 'thumb' }, img, bar, noPanel);
  const seq = h('span', { class: 'node-seq' });
  const title = h('span', { class: 'node-title' });
  const l2 = h('div', { class: 'node-l2' });
  const beadsBox = h('div', { class: 'beads' });
  const el = h('div', { class: 'node', tabindex: '0', role: 'button', dataset: { shot: id } },
    beadsBox, thumb, h('div', { class: 'node-text' }, h('div', { class: 'node-l1' }, seq, title), l2),
    h('i', { class: 'port in' }), h('i', { class: 'port out' }));
  img.addEventListener('load', () => { bar.hidden = true; img.classList.remove('stale'); });
  img.addEventListener('error', () => { bar.hidden = true; });
  return { el, img, bar, noPanel, thumb, seq, title, l2, beads: beadsBox, sig: '', url: '', shotSig: '' };
}

export function patchNode(rec, entry, badgeCtx) {
  const { shot } = entry;
  const status = shotStatus(shot);
  const width = thumbnailRenderWidth();
  const shotSig = `${state.model.signatures.get(shot.id)}/${state.sourceEpoch}/${NODE_THUMB_H}/${width}`;
  if (rec.shotSig !== shotSig) {
    rec.shotSig = shotSig;
    const take = takeOf(shot, shot.selected_take) || shot.takes[0];
    const panel = take?.panels[0];
    rec.nextUrl = panel ? panelUrl(shot.id, take.id, panel, width) : '';
  }
  const badges = nodeBadges(shot, badgeCtx);
  const sig = JSON.stringify([entry.seq, shot.title, shot.duration, status, badges, rec.nextUrl, NODE_THUMB_H]);
  if (rec.sig === sig) return;
  rec.sig = sig;
  rec.el.dataset.status = status;
  rec.el.style.height = `${NODE_H}px`;
  rec.thumb.style.height = `${NODE_THUMB_H}px`;
  rec.seq.textContent = entry.seq;
  rec.title.textContent = shot.title || 'Untitled';
  rec.title.classList.toggle('untitled', !shot.title);
  rec.el.setAttribute('aria-label', `Shot ${entry.seq} ${shot.title || 'untitled'}, ${STATUS_LABEL[status]}, ${fmtSeconds(shot.duration)}`);
  rec.l2.replaceChildren(...badges.meta.map(badgeElement));
  rec.beads.replaceChildren(...badges.top.map(badgeElement));
  setThumb(rec, rec.nextUrl);
}

/** Every registered badge's descriptors for a shot, by slot. A failing badge shows '!' on the node. */
function nodeBadges(shot, badgeCtx) {
  const out = { meta: [], top: [] };
  for (const badge of getNodeBadges()) {
    let error = null;
    let result = guard(`Node badge "${badge.id}"`, null, () => badge.render(shot, badgeCtx), (caught) => { error = caught; });
    if (error) {
      result = { text: '!', class: 'badge-error', title: `${badge.title || badge.id} failed: ${error.message}` };
    }
    for (const d of [].concat(result ?? [])) {
      if (!d) continue;
      out[badge.slot === 'top' ? 'top' : 'meta'].push({ text: String(d.text ?? ''), title: d.title, class: d.class, dot: d.dot ? true : undefined, index: d.index });
    }
  }
  return out;
}
function badgeElement(d) {
  return h('span', { class: d.class, title: d.title, dataset: d.index == null ? null : { clipIndex: d.index } }, d.text, d.dot ? h('i', { class: 'dot' }) : null);
}

/** Swaps a thumbnail without a blank flash: the old image stays at 60% until the new one is decoded. */
export function setThumb(rec, url) {
  if (rec.url === url) return;
  rec.url = url;
  rec.noPanel.hidden = !!url;
  if (!url) { rec.img.removeAttribute('src'); rec.img.hidden = true; rec.bar.hidden = true; return; }
  rec.img.hidden = false;
  rec.bar.hidden = false;
  if (!rec.img.getAttribute('src')) { rec.img.src = url; return; }
  rec.img.classList.add('stale');
  const pre = new Image();
  pre.decoding = 'async';
  pre.onload = () => { if (rec.url === url) rec.img.src = url; };
  pre.onerror = () => { if (rec.url === url) { rec.bar.hidden = true; rec.img.classList.remove('stale'); } };
  pre.src = url;
}

export function updateSelectionClasses() {
  for (const [id, rec] of nodeEls) rec.el.classList.toggle('selected', state.sel.shots.has(id));
  for (const [id, rec] of stickyEls) rec.el.classList.toggle('selected', state.sel.stickies.has(id));
  for (const [id, rec] of frameEls) rec.el.classList.toggle('selected', state.sel.scene === id);
}

export function renderStickies() {
  const { layout, model } = state;
  const seen = new Set();
  for (const [id, p] of layout.stickies) {
    const sticky = model.stickies.get(id);
    seen.add(id);
    let rec = stickyEls.get(id);
    if (!rec) {
      const el = h('div', { class: 'sticky-card', tabindex: '0', role: 'button', dataset: { sticky: id } });
      rec = { el, sig: '' };
      stickyEls.set(id, rec);
      els.stickiesLayer.append(el);
    }
    const replies = model.replies.get(id) || [];
    const author = stickyAuthor(sticky.author);
    const sig = JSON.stringify([sticky.text, sticky.kind, author, replies.length]);
    if (rec.sig !== sig) {
      rec.sig = sig;
      rec.el.classList.toggle('by-human', author.kind === 'human');
      rec.el.setAttribute('aria-label', `${KIND_LABEL[sticky.kind] || 'Note'} by ${author.name}: ${sticky.text}`);
      rec.el.replaceChildren(
        h('div', { class: 'sticky-text' }, h('span', { class: 'kind', text: KIND_GLYPH[sticky.kind] || KIND_GLYPH.note }), sticky.text),
        h('div', { class: 'sticky-meta' },
          h('span', { class: `sticky-author ${author.kind}`, text: author.name }),
          replies.length ? h('span', { text: `${replies.length} repl${replies.length === 1 ? 'y' : 'ies'}` }) : null),
      );
    }
    rec.el.style.transform = `translate(${p.x}px, ${p.y}px)`;
    rec.el.classList.toggle('selected', state.sel.stickies.has(id));
  }
  for (const [id, rec] of stickyEls) if (!seen.has(id)) { rec.el.remove(); stickyEls.delete(id); }
}

export function renderStoryWires() {
  const { model, layout } = state;
  const order = model.order;
  const list = [];
  wiresByShot.clear();
  for (let index = 1; index < order.length; index++) {
    const from = order[index - 1], to = order[index];
    const segments = wireSegments(portOut(layout.shots.get(from)), portIn(layout.shots.get(to)));
    const wire = { from, to, segments, sample: sampleSegments(segments), el: storyWires[list.length]?.el || null };
    if (!wire.el) {
      wire.el = svg('path', { class: 'wire', 'marker-end': 'url(#arrow)' });
      els.storyWires.append(wire.el);
    }
    wire.el.setAttribute('d', segmentsPath(segments));
    list.push(wire);
    for (const id of [from, to]) {
      if (!wiresByShot.has(id)) wiresByShot.set(id, []);
      wiresByShot.get(id).push(wire);
    }
  }
  for (let index = list.length; index < storyWires.length; index++) storyWires[index].el.remove();
  storyWires = list;
}

/** Redraws only the wires touching dragged shots, from their live positions. */
export function redrawWiresFor(ids, livePos) {
  const done = new Set();
  for (const id of ids) {
    for (const wire of wiresByShot.get(id) || []) {
      if (done.has(wire)) continue;
      done.add(wire);
      const a = livePos(wire.from), b = livePos(wire.to);
      wire.el.setAttribute('d', segmentsPath(wireSegments(portOut(a), portIn(b))));
    }
  }
}

export function renderCutPath() {
  els.cutWires.replaceChildren();
  const cut = state.viewCut && state.doc.cuts.find((candidate) => candidate.id === state.viewCut);
  const seen = new Set();
  if (cut) {
    const points = [];
    let previous = null;
    cut.clips.forEach((clip, index) => {
      const shotId = clip.source?.shot?.shot;
      const nestedId = clip.source?.cut?.cut;
      if (shotId && state.layout.shots.has(shotId)) {
        const p = state.layout.shots.get(shotId);
        points.push({ in: portIn(p), out: portOut(p), id: shotId });
        previous = p;
      } else if (nestedId) {
        const key = `${cut.id}/${clip.id}`;
        seen.add(key);
        const stored = state.doc.layout.positions[`cut:${nestedId}`];
        const p = stored ? { x: stored.x, y: stored.y } : previous ? { x: previous.x + NODE_W + 24, y: previous.y + NODE_H / 2 - PILL_H / 2 - 60 } : { x: 0, y: -PILL_H - 40 };
        let el = pillEls.get(key);
        const nested = state.doc.cuts.find((candidate) => candidate.id === nestedId);
        if (!el) {
          el = h('div', { class: 'cut-pill', tabindex: '0', role: 'button', dataset: { cut: nestedId } });
          pillEls.set(key, el);
          els.nodesLayer.append(el);
        }
        el.textContent = nested?.name || nestedId;
        el.title = `Clip ${index + 1}: the cut "${nested?.name || nestedId}". Double-click to view it.`;
        el.setAttribute('aria-label', `Nested cut ${nested?.name || nestedId}, clip ${index + 1}`);
        el.style.transform = `translate(${p.x}px, ${p.y}px)`;
        points.push({ in: { x: p.x, y: p.y + PILL_H / 2 }, out: { x: p.x + PILL_W, y: p.y + PILL_H / 2 }, id: key });
        previous = { x: p.x + PILL_W - NODE_W, y: p.y + PILL_H / 2 - NODE_H / 2 };
      }
    });
    for (let index = 1; index < points.length; index++) {
      if (points[index].id === points[index - 1].id) continue;
      els.cutWires.append(svg('path', { class: 'cut-wire', 'marker-end': 'url(#arrow-cut)', d: segmentsPath(wireSegments(points[index - 1].out, points[index].in)) }));
    }
  }
  for (const [key, el] of pillEls) if (!seen.has(key)) { el.remove(); pillEls.delete(key); }
}

export function flash(el, who) {
  if (!el) return;
  el.classList.remove('remote');
  void el.offsetWidth;
  el.classList.add('remote');
  setTimeout(() => el.classList.remove('remote'), 1200);
  el.querySelector('.author-chip')?.remove();
  const chip = h('span', { class: `author-chip${who.kind === 'human' ? ' human' : ''}`, text: who.name });
  el.append(chip);
  setTimeout(() => chip.remove(), 3000);
}

/** Removes every element of the open project from the graph (the project is closing or switching). */
export function dropProjectElements() {
  for (const map of [nodeEls, frameEls, stickyEls, pillEls]) { for (const rec of map.values()) (rec.el || rec).remove(); map.clear(); }
  for (const wire of storyWires) wire.el.remove();
  storyWires = [];
  els.cutWires.replaceChildren();
}

// ---------------------------------------------------------------- built-in node badges

registerNodeBadge({ id: 'duration', title: 'Duration', order: 10, render: (shot) => ({ text: fmtSeconds(shot.duration) }) });
registerNodeBadge({
  id: 'takes', title: 'Takes', order: 20,
  render: (shot) => (shot.takes.length > 1 ? { text: `${shot.takes.length} tk`, title: `${shot.takes.length} takes` } : null),
});
registerNodeBadge({
  id: 'notes', title: 'Sticky notes', order: 30,
  render: (shot, ctx) => {
    const pinned = ctx.state.model.pinnedToShot.get(shot.id) || [];
    if (!pinned.length) return null;
    const open = pinned.filter((s) => !s.resolved).length;
    return { class: 'stk', title: `${pinned.length} sticky notes, ${open} open`, text: `${pinned.length} note${pinned.length === 1 ? '' : 's'}`, dot: open > 0 };
  },
});
registerNodeBadge({
  id: 'clips', title: 'Clip numbers in the viewed cut', order: 40, slot: 'top',
  render: (shot, ctx) => ctx.clips.map((n) => ({ class: 'bead', text: String(n), index: n - 1, title: `Drag clip ${n} to another numbered bead to reorder` })),
});

// ---------------------------------------------------------------- the panel

registerPanel({
  id: 'graph', title: 'Shot graph', region: 'graph', order: 10, dock: { defaultDock: 'center', shortcut: 'Mod+Alt+1' },
  mount(el, ctx) {
    el.innerHTML = GRAPH_HTML;
    const q = (selector) => $(selector, el);
    Object.assign(els, {
      graph: el.closest('[data-region]'), world: q('#world'), framesLayer: q('#frames-layer'), nodesLayer: q('#nodes-layer'),
      stickiesLayer: q('#stickies-layer'), wires: q('#wires'), storyWires: q('#story-wires'), cutWires: q('#cut-wires'),
      dropMarker: q('#drop-marker'), dropGap: q('#drop-gap'), dropCapA: q('#drop-cap-a'), dropCapB: q('#drop-cap-b'),
      marquee: q('#marquee'), chip: q('#drag-chip'), graphEmpty: q('#graph-empty'),
    });
    // the drag chip follows the cursor over both the graph and the inspector
    document.body.append(els.chip);
    els.chip.style.position = 'fixed';
    installViewport(ctx, onViewChanged);
    installDrag(ctx);
    ctx.store.on('doc', (info) => {
      if (!state.doc) return;
      renderGraph();
      if (!info.initial && info.old && info.old.scenes.length === 0 && state.doc.scenes.length > 0) frameBox(worldBounds(null), 1);
    });
    ctx.store.on('selection', (info) => {
      updateSelectionClasses();
      if (info?.focus && info.type === 'shot') nodeEls.get(info.id)?.el.focus({ preventScroll: true });
    });
    ctx.store.on('filter', () => renderNodes(!!state.viewCut));
    ctx.store.on('remote', ({ who, shots, stickies }) => {
      for (const id of shots) flash(nodeEls.get(id)?.el, who);
      for (const id of stickies) flash(stickyEls.get(id)?.el, who);
    });
    ctx.store.on('projectDrop', dropProjectElements);
    ctx.store.on('projectClosed', renderGraphEmpty);
    ctx.store.on('workspace', () => { if (!state.project) renderGraphEmpty(); });
    ctx.store.on('renameScene', (id) => requestAnimationFrame(() => startRenameScene(id)));
    return {};
  },
});
