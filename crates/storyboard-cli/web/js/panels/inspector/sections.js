// The built-in sections of the shot inspector, in order: header and preview (pinned), panels,
// takes, fields, layers. Stickies add theirs in stickies.js.

import { setStatusFor } from '../../core/actions.js';
import { commit, panelUrl, undo, uploadShotImage } from '../../core/api.js';
import { boardImageOp } from '../../core/board-image.js';
import { layerDragPatch, layerHit } from '../../core/layer-edit.js';
import { panelInsertionOps, panelRetimeOps } from '../../core/panel-insert.js';
import { els, h, ICON_EYE_OFF, ICON_PLUS, iconButton, preserveFocus } from '../../core/dom.js';
import { shotStatus, STATUS_LABEL, STATUSES, takeOf } from '../../core/model.js';
import { openMenu } from '../../core/popover.js';
import { registerCommand, registerInspectorSection } from '../../core/registry.js';
import { emit, notify, setShotUi, shotEntry, shotView, state } from '../../core/store.js';
import { contentDuration } from '../../core/timing.js';
import { clamp, fmtSeconds } from '../../core/util.js';
import { NODE_THUMB_H } from '../graph/layout.js';
import { commitFocusedField, makeField, syncField, updateFramesLabel } from './fields.js';
import { renderInspector } from './index.js';
import { insp } from './shared.js';

export function patchHeader(entry) {
  const { seq, status } = insp.parts;
  seq.textContent = entry.seq;
  const value = shotStatus(entry.shot);
  status.textContent = STATUS_LABEL[value];
  status.style.setProperty('--sw', `var(--status-${value})`);
  status.setAttribute('aria-label', `Status: ${STATUS_LABEL[value]}. Change status`);
}

// ---------------------------------------------------------------- preview

export function previewWidth() {
  const width = Math.max(120, els.inspector.clientWidth - 24);
  return clamp(Math.round(width * (window.devicePixelRatio || 1)), 16, 4096);
}

export function patchPreview(entry, view) {
  const parts = insp.parts;
  const format = state.doc.format;
  parts.preview.style.aspectRatio = `${format.width} / ${format.height}`;
  if (!view.take || !view.panel) {
    parts.previewImg.hidden = true;
    parts.previewBar.hidden = true;
    parts.warn.hidden = true;
    insp.previewUrl = '';
    parts.previewEmpty.hidden = false;
    parts.previewEmpty.replaceChildren(
      h('span', { text: view.take ? `${view.take.name || 'This take'} has no panels.` : 'This shot has no takes.' }),
      view.take ? h('button', { class: 'btn small', type: 'button', text: 'Add panel', onclick: () => addPanel(entry.shot.id, view.take.id, 0) }) : null,
    );
    return;
  }
  parts.previewEmpty.hidden = true;
  parts.previewImg.hidden = false;
  parts.preview.setAttribute('aria-label', `Panel ${view.take.panels.indexOf(view.panel) + 1} of ${view.take.name || 'take'}`);
  loadPreview(panelUrl(entry.shot.id, view.take.id, view.panel, previewWidth()));
}

export async function loadPreview(url) {
  if (insp.previewUrl === url) return;
  insp.previewUrl = url;
  const parts = insp.parts;
  parts.previewBar.hidden = false;
  if (parts.previewImg.getAttribute('src')) parts.previewImg.classList.add('stale');
  let response;
  try { response = await fetch(url); } catch { response = null; }
  if (insp.previewUrl !== url || !insp.parts) return;
  if (!response || !response.ok) {
    let message = 'The panel could not be rendered.';
    try { const body = await response.json(); message = body.error?.message || message; } catch { /* not JSON */ }
    parts.previewBar.hidden = true;
    parts.previewImg.classList.remove('stale');
    parts.warn.hidden = false;
    parts.warn.textContent = message;
    return;
  }
  let warnings = [];
  try { warnings = JSON.parse(response.headers.get('X-Render-Warnings') || '[]'); } catch { warnings = []; }
  const blob = await response.blob();
  if (insp.previewUrl !== url || !insp.parts) return;
  const objectUrl = URL.createObjectURL(blob);
  const img = parts.previewImg;
  img.onload = () => {
    if (insp.previewObjectUrl && insp.previewObjectUrl !== objectUrl) URL.revokeObjectURL(insp.previewObjectUrl);
    insp.previewObjectUrl = objectUrl;
    img.classList.remove('stale');
    parts.previewBar.hidden = true;
  };
  img.src = objectUrl;
  const text = (Array.isArray(warnings) ? warnings : []).map((w) => (typeof w === 'string' ? w : w.message || JSON.stringify(w))).join('; ');
  parts.warn.hidden = !text;
  parts.warn.textContent = text;
}

// ---------------------------------------------------------------- panel strip

export function patchStrip(entry, view) {
  const { strip, panelFields } = insp.parts;
  const shot = entry.shot;
  const take = view.take;
  const thumbs = take ? take.panels.map((panel) => [panel.id, panel.time, panelUrl(shot.id, take.id, panel, 144)]) : [];
  const sig = JSON.stringify([take?.id, thumbs, view.panel?.id, NODE_THUMB_H]);
  if (insp.sigs.strip === sig) return;
  insp.sigs.strip = sig;
  preserveFocus(strip, () => {
    for (const child of [...strip.children]) if (child !== panelFields) child.remove();
    if (!take) return;
    const head = h('div', { class: 'section-head' }, 'Panels', h('span', { class: 'count', text: String(take.panels.length) }));
    const row = h('div', { class: 'strip', role: 'listbox', 'aria-label': 'Panels' });
    take.panels.forEach((panel, index) => {
      const selected = panel.id === view.panel?.id;
      const item = h('button', {
        class: `strip-item${selected ? ' selected' : ''}`, type: 'button', role: 'option', 'aria-selected': String(selected),
        'aria-label': `Panel ${index + 1}, ${fmtSeconds(panel.time)}`,
        dataset: { key: `strip:${panel.id}`, pin: `panel:${shot.id}/${take.id}/${panel.id}`, pinLabel: `panel ${index + 1}`, reorderKind: 'panel', reorderId: panel.id },
      },
      h('div', { class: 'st-img', style: { height: `${Math.round(72 * state.doc.format.height / state.doc.format.width)}px` } },
        h('img', { alt: '', loading: 'lazy', decoding: 'async', src: panelUrl(shot.id, take.id, panel, 144), draggable: 'false' })),
      h('span', { class: 'st-time', text: fmtSeconds(panel.time) }));
      item.addEventListener('click', () => selectPanel(shot.id, panel.id));
      item.addEventListener('keydown', (event) => {
        if (event.key === 'Delete' || event.key === 'Backspace') {
          event.preventDefault();
          event.stopPropagation();
          commit([{ remove_panel: { shot: shot.id, take: take.id, panel: panel.id } }], { what: 'Remove panel' });
        }
      });
      row.append(item);
    });
    const add = h('button', { class: 'strip-add', type: 'button', html: ICON_PLUS, 'aria-label': 'Add panel after the selected one', title: 'Add panel after the selected one' });
    const selectedIndex = take.panels.findIndex((panel) => panel.id === view.panel?.id);
    add.addEventListener('click', () => addPanel(shot.id, take.id, selectedIndex + 1));
    row.append(add);
    strip.insertBefore(head, panelFields);
    strip.insertBefore(row, panelFields);
  });
}

export function selectPanel(shotId, panelId) {
  commitFocusedField();
  const entry = shotEntry(shotId);
  if (!entry) return;
  shotView(entry.shot);
  setShotUi(shotId, { panel: panelId });
  renderInspector();
}
export function stepPanel(delta) {
  const entry = state.primary?.type === 'shot' && shotEntry(state.primary.id);
  if (!entry) return;
  const { take, panel } = shotView(entry.shot);
  if (!take || !take.panels.length) return;
  const index = clamp(take.panels.indexOf(panel) + delta, 0, take.panels.length - 1);
  selectPanel(entry.shot.id, take.panels[index].id);
}

export function addPanel(shotId, takeId, index) {
  const shot = shotEntry(shotId)?.shot;
  const take = shot && takeOf(shot, takeId);
  if (!take) return;
  let ops;
  try { ops = panelInsertionOps(shot, take, index, state.doc.fps); }
  catch (error) { notify.error(error, 'Adding panel'); return; }
  commit(ops, {
    what: 'Add panel',
    onDone: (result) => {
      const panelId = (result.ids || []).find((id) => id.startsWith('panel-'));
      if (panelId) { setShotUi(shotId, { take: takeId, panel: panelId }); renderInspector(); }
    },
  });
}

export function patchPanelFields(entry, view) {
  const box = insp.parts.panelFields;
  const panel = view.panel;
  const key = panel ? `${entry.shot.id}/${view.take.id}/${panel.id}` : '';
  if (box.dataset.panel !== key) {
    box.dataset.panel = key;
    for (const [fieldKey] of insp.fields) if (fieldKey.startsWith('panel:')) insp.fields.delete(fieldKey);
    box.replaceChildren();
    if (!panel) return;
    const shotId = entry.shot.id, takeId = view.take.id, panelId = panel.id;
    const currentPanel = () => takeOf(shotEntry(shotId)?.shot || { takes: [] }, takeId)?.panels.find((p) => p.id === panelId);
    const time = makeField({
      key: `panel:${key}:time`, label: 'Time', number: true,
      get: () => { const p = currentPanel(); return p ? String(Math.round((p.time || 0) * 1000) / 1000) : ''; },
      toOps: (value) => {
        if (!value.trim()) return { error: 'Enter seconds from the shot start, 0 or more.' };
        const shot = shotEntry(shotId)?.shot;
        const take = shot && takeOf(shot, takeId);
        if (!take) return { error: 'The take changed while editing this panel.' };
        try { return { ops: panelRetimeOps(shot, take, panelId, Number(value), state.doc.fps) }; }
        catch (error) { return { error: error.message }; }
      },
    });
    const direction = makeField({
      key: `panel:${key}:direction`, label: 'Direction',
      get: () => currentPanel()?.direction || '',
      toOps: (value) => ({ ops: [{ update_panel: { shot: shotId, take: takeId, panel: panelId, patch: { direction: value } } }] }),
    });
    for (const rec of [time, direction]) { box.append(h('label', { for: rec.el.id, text: rec.label }), rec.wrap); syncField(rec); }
    const picker = h('input', { type: 'file', accept: '.png,.jpg,.jpeg,.webp,.gif,.tif,.tiff,.svg', hidden: true });
    const replace = h('button', { class: 'btn small', type: 'button', text: 'Replace board image',
      title: 'Replace this panel’s full-size board image; keep other layers and Undo history' });
    replace.addEventListener('click', () => picker.click());
    picker.addEventListener('change', async () => {
      const file = picker.files?.[0];
      const project = state.project;
      const base = state.version;
      if (!file || !project) return;
      if (!/\.(png|jpe?g|webp|gif|tiff?|svg)$/i.test(file.name) || !file.size || file.size > 8 * 1024 * 1024) {
        notify.status({ text: 'Choose a nonempty PNG, JPEG, WebP, GIF, TIFF or SVG under 8 MB.', error: true });
        picker.value = '';
        return;
      }
      replace.disabled = true;
      try {
        const uploaded = await uploadShotImage(project, crypto.randomUUID(), file);
        if (state.project !== project) throw new Error('Project changed during image upload.');
        const current = takeOf(shotEntry(shotId)?.shot || { takes: [] }, takeId)?.panels.find((p) => p.id === panelId);
        if (!current) throw new Error('Panel changed during image upload.');
        await commit([boardImageOp(shotId, takeId, current, state.doc.format, uploaded.path)],
          { base, what: 'Replace board image', label: 'Replaced board image' });
      } catch (error) { notify.error(error, 'Replacing board image'); }
      finally { replace.disabled = false; picker.value = ''; }
    });
    box.append(h('div', { class: 'panel-image-actions' }, replace, picker));
  }
}

// ---------------------------------------------------------------- takes

export function patchTakes(entry, view) {
  const box = insp.parts.takes;
  const shot = entry.shot;
  const sig = JSON.stringify([shot.takes.map((t) => [t.id, t.name, t.panels.length]), shot.selected_take, view.take?.id]);
  if (insp.sigs.takes === sig) return;
  insp.sigs.takes = sig;
  preserveFocus(box, () => {
    box.replaceChildren();
    const tabs = h('div', { class: 'takes', role: 'tablist', 'aria-label': 'Takes' });
    shot.takes.forEach((take, index) => {
      const viewed = take.id === view.take?.id;
      const selected = take.id === shot.selected_take;
      const tab = h('button', {
        class: `take-tab${viewed ? ' viewed' : ''}`, type: 'button', role: 'tab', 'aria-selected': String(viewed),
        title: `${take.name || `Take ${index + 1}`}${selected ? ' (the take this shot uses)' : ''}. Right-click for more.`,
        dataset: { key: `take:${take.id}`, pin: `take:${shot.id}/${take.id}`, pinLabel: `take ${take.name || index + 1}`, reorderKind: 'take', reorderId: take.id },
      }, selected ? h('i', { class: 'sel-dot', 'aria-label': 'used' }) : null, h('span', { text: take.name || `Take ${index + 1}` }));
      tab.addEventListener('click', () => { commitFocusedField(); setShotUi(shot.id, { take: take.id, panel: null }); renderInspector(); });
      tab.addEventListener('contextmenu', (event) => { event.preventDefault(); takeMenu(shot.id, take, { x: event.clientX, y: event.clientY }); });
      tabs.append(tab);
    });
    const add = iconButton('Add a blank take', ICON_PLUS, () => {
      commit([{ add_take: { shot: shot.id, name: `Take ${shot.takes.length + 1}` } }], {
        what: 'Add take',
        onDone: (result) => {
          const takeId = (result.ids || []).find((id) => id.startsWith('take-'));
          if (takeId) { setShotUi(shot.id, { take: takeId, panel: null }); renderInspector(); }
        },
      });
    });
    tabs.append(add);
    box.append(h('div', { class: 'section-head' }, 'Takes', h('span', { class: 'count', text: String(shot.takes.length) })), tabs);
    const actions = h('div', { class: 'take-actions' });
    if (view.take && view.take.id !== shot.selected_take) {
      actions.append(h('button', {
        class: 'btn primary small', type: 'button', text: `Use ${view.take.name || 'this take'}`,
        onclick: () => commit([{ select_take: { shot: shot.id, take: view.take.id } }], { what: 'Use take' }),
      }), h('span', { class: 'preview-warn', style: { margin: 0 }, text: 'Previewing. Nothing changes until you use it.' }));
    }
    if (view.take) {
      const more = h('button', { class: 'btn small', type: 'button', text: 'Take menu', 'aria-haspopup': 'menu', title: `Duplicate or remove ${view.take.name || 'this take'}` });
      more.addEventListener('click', () => { const r = more.getBoundingClientRect(); takeMenu(shot.id, view.take, { x: r.left, y: r.bottom + 4 }, more); });
      tabs.append(more);
    }
    if (actions.childElementCount) box.append(actions);
  });
}

export function takeMenu(shotId, take, at, anchor) {
  openMenu([
    { label: `Duplicate ${take.name || 'take'}`, run: () => commit([{ duplicate_take: { shot: shotId, take: take.id } }], { what: 'Duplicate take' }) },
    { label: `Remove ${take.name || 'take'}`, danger: true, run: () => commit([{ remove_take: { shot: shotId, take: take.id } }], { what: 'Remove take', label: `removed ${take.name || take.id}. Cmd+Z to undo` }) },
  ], at, anchor);
}

// ---------------------------------------------------------------- layers

export const LAYER_GLYPH = { image: '▣', text: 'T', solid: '■', box: '□' };
export function patchLayers(entry, view) {
  const box = insp.parts.layers;
  const layers = view.panel ? [...view.panel.layers].reverse() : [];
  if (insp.layerPanel !== view.panel?.id) { insp.layerPanel = view.panel?.id || null; insp.selectedLayer = null; }
  const sig = JSON.stringify([view.panel?.id, layers, insp.selectedLayer]);
  if (insp.sigs.layers === sig) return;
  insp.sigs.layers = sig;
  box.hidden = !view.panel;
  box.replaceChildren(h('summary', { class: 'section-head' }, h('span', { class: 'tw', text: '›' }), 'Layers', h('span', { class: 'count', text: String(layers.length) })));
  if (!view.panel) return;
  const shot = entry.shot.id, take = view.take.id, panel = view.panel.id;
  const caption = h('input', { type: 'text', maxlength: 20000, placeholder: 'Text on this board', 'aria-label': 'New board caption' });
  const add = h('button', { class: 'btn small', type: 'button', text: 'Add text' });
  add.addEventListener('click', async () => {
    const text = caption.value.trim();
    if (!text) { caption.focus(); return; }
    const { width, height } = state.doc.format;
    add.disabled = true;
    try {
      await commit([{ add_layer: { shot, take, panel, layer_type: 'text', fields: {
        text, x: width * 0.05, y: height * 0.05, width: width * 0.9, height: height * 0.24,
        font_size: Math.max(20, Math.round(width * 0.05)), weight: 'bold', color: '#ffd22b',
      } } }], { what: 'Add board text' });
    } catch (error) { notify.error(error, 'Adding board text'); }
    finally { add.disabled = false; }
  });
  box.append(h('div', { class: 'layer-add' }, caption, add));
  if (!layers.length) { box.append(h('div', { class: 'preview-warn', style: { margin: 0 }, text: 'No layers on this panel.' })); return; }
  for (const layer of layers) {
    const label = layer.label || (layer.type === 'text' ? layer.text : layer.type === 'image' ? layer.src : '') || layer.type;
    const row = h('div', { class: `layer-row${layer.hidden ? ' hidden-layer' : ''}${insp.selectedLayer === layer.id ? ' selected' : ''}` },
      h('span', { class: 'glyph', text: LAYER_GLYPH[layer.type] || '?', title: layer.type }),
      h('span', { class: 'lbl', text: label, title: label }),
      layer.hidden ? h('span', { html: ICON_EYE_OFF, title: 'Hidden', 'aria-label': 'Hidden' }) : null);
    row.addEventListener('click', () => selectLayer(layer.id));
    row.addEventListener('pointerenter', () => showLayerBox(layer));
    row.addEventListener('pointerleave', () => { insp.parts.layerBox.hidden = true; });
    box.append(row);
    const opacity = h('input', { type: 'range', min: '0', max: '1', step: '0.05', value: String(layer.opacity ?? 1), 'aria-label': `Opacity for ${label}` });
    const controls = h('div', { class: 'layer-controls' },
      h('label', { text: 'Opacity' }, opacity),
      h('button', { class: 'btn small', type: 'button', text: layer.hidden ? 'Show' : 'Hide' }),
      h('button', { class: 'btn small', type: 'button', text: 'Up', disabled: layer === view.panel.layers[view.panel.layers.length - 1] }),
      h('button', { class: 'btn small', type: 'button', text: 'Down', disabled: layer === view.panel.layers[0] }));
    const [, visible, up, down] = controls.children;
    opacity.addEventListener('change', () => commitLayer({ update_layer: { shot, take, panel, layer: layer.id, patch: { opacity: Number(opacity.value) } } }, 'Change layer opacity'));
    visible.addEventListener('click', (event) => { event.stopPropagation(); commitLayer({ update_layer: { shot, take, panel, layer: layer.id, patch: { hidden: !layer.hidden } } }, 'Toggle layer visibility'); });
    up.addEventListener('click', (event) => { event.stopPropagation(); const index = view.panel.layers.indexOf(layer); commitLayer({ move_layer: { shot, take, panel, layer: layer.id, index: index + 1 } }, 'Move layer up'); });
    down.addEventListener('click', (event) => { event.stopPropagation(); const index = view.panel.layers.indexOf(layer); commitLayer({ move_layer: { shot, take, panel, layer: layer.id, index: index - 1 } }, 'Move layer down'); });
    box.append(controls);
    if (layer.type === 'text') {
      const input = h('input', { type: 'text', value: layer.text || '', maxlength: 20000,
        'aria-label': `Edit text for ${label}` });
      const save = h('button', { class: 'btn small', type: 'button', text: 'Save' });
      save.addEventListener('click', async () => {
        if (input.value === layer.text) return;
        save.disabled = true;
        try { await commitLayer({ update_layer: { shot, take, panel, layer: layer.id, patch: { text: input.value } } }, 'Edit board text'); }
        catch (error) { notify.error(error, 'Editing board text'); }
        finally { save.disabled = false; }
      });
      const remove = h('button', { class: 'btn small', type: 'button', text: 'Remove' });
      remove.addEventListener('click', () => commit([{ remove_layer: { shot, take, panel, layer: layer.id } }],
        { what: 'Remove board text' }));
      box.append(h('div', { class: 'layer-text-edit' }, input, save, remove));
    }
  }
}
export function commitLayer(op, what) {
  return commit([op], { what, onDone: () => { insp.previewUrl = ''; renderInspector(); } });
}
export function selectLayer(id) {
  insp.selectedLayer = id;
  const entry = state.primary?.type === 'shot' && shotEntry(state.primary.id);
  if (!entry) return;
  const view = shotView(entry.shot);
  patchLayerEditor(view);
  patchLayers(entry, view);
}
export function patchLayerEditor(view) {
  const editor = insp.parts.layerEditor;
  if (!editor || !view.panel) { if (editor) editor.hidden = true; return; }
  const layer = view.panel.layers.find((candidate) => candidate.id === insp.selectedLayer);
  if (!layer || layer.hidden) { editor.hidden = true; return; }
  const { width, height } = state.doc.format;
  Object.assign(editor.style, { left: `${layer.x / width * 100}%`, top: `${layer.y / height * 100}%`, width: `${layer.width / width * 100}%`, height: `${layer.height / height * 100}%` });
  editor.hidden = false;
}
export function installLayerEditor(preview) {
  preview.addEventListener('pointerdown', (event) => {
    const entry = state.primary?.type === 'shot' && shotEntry(state.primary.id);
    if (!entry) return;
    const view = shotView(entry.shot);
    if (!view.panel) return;
    const rect = preview.getBoundingClientRect();
    const point = { x: (event.clientX - rect.left) * state.doc.format.width / rect.width, y: (event.clientY - rect.top) * state.doc.format.height / rect.height };
    const handle = event.target.closest('[data-layer-handle]')?.dataset.layerHandle || 'move';
    const layer = handle === 'move' ? layerHit(view.panel.layers, point.x, point.y)
      : view.panel.layers.find((candidate) => candidate.id === insp.selectedLayer);
    if (!layer || (handle === 'move' && !layerHit(view.panel.layers, point.x, point.y))) return;
    event.preventDefault(); event.stopPropagation(); insp.selectedLayer = layer.id; patchLayerEditor(view); patchLayers(entry, view);
    const start = { x: event.clientX, y: event.clientY, layer: { ...layer } };
    preview.setPointerCapture(event.pointerId);
    const move = (next) => {
      const dx = (next.clientX - start.x) * state.doc.format.width / rect.width;
      const dy = (next.clientY - start.y) * state.doc.format.height / rect.height;
      const patch = layerDragPatch(start.layer, handle, dx, dy);
      const draft = { ...start.layer, ...patch };
      Object.assign(insp.parts.layerEditor.style, { left: `${draft.x / state.doc.format.width * 100}%`, top: `${draft.y / state.doc.format.height * 100}%`, width: `${draft.width / state.doc.format.width * 100}%`, height: `${draft.height / state.doc.format.height * 100}%` });
      start.patch = patch;
    };
    const done = () => {
      preview.removeEventListener('pointermove', move); preview.removeEventListener('pointerup', done); preview.removeEventListener('pointercancel', done);
      if (start.patch && Object.keys(start.patch).some((key) => start.patch[key] !== start.layer[key])) commitLayer({ update_layer: { shot: entry.shot.id, take: view.take.id, panel: view.panel.id, layer: layer.id, patch: start.patch } }, 'Move layer');
    };
    preview.addEventListener('pointermove', move); preview.addEventListener('pointerup', done); preview.addEventListener('pointercancel', done);
  });
}
export function showLayerBox(layer) {
  const { width, height } = state.doc.format;
  const boxEl = insp.parts.layerBox;
  Object.assign(boxEl.style, {
    left: `${(layer.x / width) * 100}%`, top: `${(layer.y / height) * 100}%`,
    width: `${(layer.width / width) * 100}%`, height: `${(layer.height / height) * 100}%`,
  });
  boxEl.hidden = false;
}

// ---------------------------------------------------------------- registrations

registerInspectorSection({
  id: 'header', title: 'Header', order: 10, pinned: true,
  mount(el, ctx) {
    const shotId = ctx.shotId;
    const seq = h('span', { class: 'insp-seq' });
    const title = makeField({
      key: `${shotId}:title`, label: 'Title', className: 'insp-title', bare: true,
      get: () => shotEntry(shotId)?.shot.title || '',
      toOps: (value) => ({ ops: [{ update_shot: { shot: shotId, patch: { title: value.trim() } } }] }),
    });
    const status = h('button', { class: 'status-pill', type: 'button', 'aria-haspopup': 'menu', title: 'Status (keys 1 to 4)' });
    status.addEventListener('click', () => {
      const rect = status.getBoundingClientRect();
      openMenu(STATUSES.map((value, index) => ({
        label: STATUS_LABEL[value], key: String(index + 1), status: value, on: shotStatus(shotEntry(shotId).shot) === value,
        run: () => setStatusFor([shotId], value),
      })), { x: rect.left, y: rect.bottom + 4 }, status);
    });
    const header = h('div', { class: 'insp-header' }, seq, title.wrap, status);
    el.append(header);
    Object.assign(insp.parts, { seq, status });
    return { update(entry) { patchHeader(entry); syncField(title); } };
  },
});

registerInspectorSection({
  id: 'preview', title: 'Preview', order: 20, pinned: true,
  mount(el) {
    const previewImg = h('img', { alt: '' });
    const previewBar = h('div', { class: 'bar', hidden: true });
    const previewEmpty = h('div', { class: 'empty-msg', hidden: true });
    const layerBox = h('div', { class: 'layer-box', hidden: true });
    const layerEditor = h('div', { class: 'layer-editor', hidden: true }, ...['nw', 'ne', 'sw', 'se'].map((handle) => h('i', { class: `layer-handle ${handle}`, dataset: { layerHandle: handle } })));
    const preview = h('div', { class: 'preview', tabindex: '0', role: 'img', title: 'Double-click or press Enter to review the panel full size' }, previewImg, layerBox, layerEditor, previewBar, previewEmpty);
    preview.addEventListener('dblclick', () => emit('review'));
    preview.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); emit('review'); } });
    const warn = h('div', { class: 'preview-warn', hidden: true });
    el.append(preview, warn);
    Object.assign(insp.parts, { preview, previewImg, previewBar, previewEmpty, layerBox, layerEditor, warn });
    installLayerEditor(preview);
    return { update(entry, view) { patchPreview(entry, view); patchLayerEditor(view); } };
  },
});

registerInspectorSection({
  id: 'panels', title: 'Panels', order: 30,
  mount(el) {
    const strip = h('div', { class: 'section' });
    const panelFields = h('div', { class: 'panel-fields' });
    strip.append(panelFields);
    el.append(strip);
    Object.assign(insp.parts, { strip, panelFields });
    return {
      update(entry, view) {
        patchStrip(entry, view);
        for (const [key, rec] of insp.fields) if (key.startsWith('panel:')) syncField(rec);
        patchPanelFields(entry, view);
      },
    };
  },
});

registerInspectorSection({
  id: 'takes', title: 'Takes', order: 40,
  mount(el) {
    const takes = h('div', { class: 'section' });
    el.append(takes);
    insp.parts.takes = takes;
    return { update(entry, view) { patchTakes(entry, view); } };
  },
});

registerInspectorSection({
  id: 'fields', title: 'Shot fields', order: 50,
  mount(el, ctx) {
    const shotId = ctx.shotId;
    const fields = h('div', { class: 'section' });
    const fieldsGrid = h('div', { class: 'fields' });
    fields.append(fieldsGrid);
    const textField = (name, label, multiline) => makeField({
      key: `${shotId}:${name}`, label, multiline,
      get: () => shotEntry(shotId)?.shot[name] || '',
      toOps: (value) => ({ ops: [{ update_shot: { shot: shotId, patch: { [name]: value } } }] }),
    });
    const durationField = makeField({
      key: `${shotId}:duration`, label: 'Duration', number: true,
      get: () => { const d = shotEntry(shotId)?.shot.duration; return d === undefined ? '' : String(Math.round(d * 1000) / 1000); },
      toOps: (value) => {
        const seconds = Number(value);
        if (!value.trim() || !Number.isFinite(seconds) || seconds <= 0) return { error: 'Enter the length in seconds, above 0.' };
        return { ops: [{ update_shot: { shot: shotId, patch: { duration: seconds } } }] };
      },
    });
    const frames = h('span', { class: 'frames' });
    durationField.framesEl = frames;
    durationField.el.addEventListener('input', () => updateFramesLabel(durationField));
    const durationRow = h('div', { class: 'duration-row' }, durationField.el, frames);
    durationField.wrap.prepend(durationRow);
    const rows = [
      textField('action', 'Action', true), textField('dialogue', 'Dialogue', true), textField('speaker', 'Speaker'),
      textField('camera', 'Camera'), durationField, textField('notes', 'Shot notes', true),
    ];
    for (const rec of rows) fieldsGrid.append(h('label', { for: rec.el.id, text: rec.label }), rec.wrap);
    const sourcePath = h('code', { class: 'import-shot-muted' });
    const sourceBox = h('div', { class: 'fields', hidden: true }, h('strong', { text: 'Original movie range' }), sourcePath);
    const sourceFields = ['start', 'end'].map((edge) => makeField({
      key: `${shotId}:source_movie:${edge}`, label: edge === 'start' ? 'Source in (s)' : 'Source out (s)', number: true,
      get: () => { const value = shotEntry(shotId)?.shot.source_movie?.[edge]; return value === undefined ? '' : String(Math.round(value * 1000) / 1000); },
      toOps: (value) => {
        const movie = shotEntry(shotId)?.shot.source_movie;
        const seconds = Number(value);
        if (!movie || !value.trim() || !Number.isFinite(seconds) || seconds < 0) return { error: 'Enter a source time of 0 seconds or more.' };
        const range = { ...movie, [edge]: seconds };
        if (range.end <= range.start) return { error: 'Source out must be after source in.' };
        const shot = shotEntry(shotId)?.shot;
        return { ops: [{ update_shot: { shot: shotId, patch: { source_movie: range, duration: contentDuration(shot, state.doc.fps, range) } } }] };
      },
    }));
    for (const rec of sourceFields) sourceBox.append(h('label', { for: rec.el.id, text: rec.label }), rec.wrap);
    const audioPath = h('code', { class: 'import-shot-muted' });
    const audioBox = h('div', { class: 'fields', hidden: true }, h('strong', { text: 'Attached voice-over' }), audioPath);
    const audioFields = ['start', 'end'].map((edge) => makeField({
      key: `${shotId}:source_audio:${edge}`, label: edge === 'start' ? 'Audio in (s)' : 'Audio out (s)', number: true,
      get: () => { const value = shotEntry(shotId)?.shot.source_audio?.[edge]; return value === undefined ? '' : String(Math.round(value * 1000) / 1000); },
      toOps: (value) => {
        const audio = shotEntry(shotId)?.shot.source_audio;
        const seconds = Number(value);
        if (!audio || !value.trim() || !Number.isFinite(seconds) || seconds < 0) return { error: 'Enter an audio source time of 0 seconds or more.' };
        const range = { ...audio, [edge]: seconds };
        if (range.end <= range.start) return { error: 'Audio out must be after audio in.' };
        const shot = shotEntry(shotId)?.shot;
        return { ops: [{ update_shot: { shot: shotId, patch: { source_audio: range, duration: contentDuration(shot, state.doc.fps, shot.source_movie, range) } } }] };
      },
    }));
    for (const rec of audioFields) audioBox.append(h('label', { for: rec.el.id, text: rec.label }), rec.wrap);
    const detachAudio = h('button', { class: 'btn', type: 'button', text: 'Detach voice-over' });
    detachAudio.addEventListener('click', () => {
      const shot = shotEntry(shotId)?.shot;
      if (!shot?.source_audio) return;
      commit([{ update_shot: { shot: shotId, patch: { source_audio: null, duration: contentDuration(shot, state.doc.fps, shot.source_movie, null) } } }], { what: 'Detach voice-over' });
    });
    audioBox.append(detachAudio);
    fields.append(sourceBox, audioBox);
    el.append(fields);
    return { update() {
      for (const rec of rows) syncField(rec);
      const movie = shotEntry(shotId)?.shot.source_movie;
      sourceBox.hidden = !movie;
      sourcePath.textContent = movie?.src || '';
      if (movie) for (const rec of sourceFields) syncField(rec);
      const audio = shotEntry(shotId)?.shot.source_audio;
      audioBox.hidden = !audio;
      audioPath.textContent = audio?.src || '';
      if (audio) for (const rec of audioFields) syncField(rec);
    } };
  },
});

registerInspectorSection({
  id: 'layers', title: 'Layers', order: 60,
  mount(el) {
    const layers = h('details', { class: 'section layers' });
    layers.open = insp.layersOpen;
    layers.addEventListener('toggle', () => { insp.layersOpen = layers.open; });
    el.append(layers);
    insp.parts.layers = layers;
    return { update(entry, view) { patchLayers(entry, view); } };
  },
});

registerCommand({
  id: 'panel.step', title: 'Previous or next panel', keys: ['[', ']'],
  run: (ctx, event) => stepPanel(event?.key === '[' ? -1 : 1),
  keymap: { order: 120, keys: '[ ]', label: 'Previous or next panel' },
});
