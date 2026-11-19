// The single owner of app state: the document, version, selection and UI state, plus a small event bus.
// Everything else reads `state` (a read-only view) and changes it only through the functions here or
// through commit()/undo() in api.js.

import { buildModel, normalize, takeOf } from './model.js';
import { freezeDocument } from './document-freeze.js';
import { eventAuthor } from './util.js';
import { computeLayout, setFormat } from '../panels/graph/layout.js';

const S = {
  workspace: null,          // /api/workspace result
  project: null,            // open project folder name
  version: null,
  doc: null,                // document currently drawn (may be optimistic)
  confirmed: null,          // last document the server returned
  sequence: null,
  viewCut: null,            // cut id being viewed, or null for story order
  model: null,              // indexes over doc (see buildModel)
  layout: null,             // positions (see computeLayout)
  view: { x: 40, y: 40, z: 1 },
  sel: { shots: new Set(), stickies: new Set(), scene: null },
  primary: null,            // {type: 'shot'|'sticky'|'scene', id}
  shotUi: new Map(),        // shot id -> {take: viewed take id, panel: selected panel id}
  conn: 'connecting',
  offline: false,
  dragging: false,
  deferred: null,           // remote document that arrived during a drag
  myCommits: [],            // summaries of my commits, newest last, for naming undo
  sourceEpoch: 0,
  showResolved: false,
  removedBy: null,          // name of whoever removed the selected shot
  loading: false,
  applyEpoch: 0,
  pointer: null,            // last pointer position over the graph, client coords
  shotFilter: null,         // Set of graph shots matching the toolbar filter, or null
  remoteName: null,         // author of the remote change being drawn, while it is drawn
};

/** Read-only view of the state. Assigning a top-level field throws; use the mutators below. */
export const state = new Proxy(S, {
  set(_, key) { throw new Error(`state.${String(key)} is read-only: change it through store.js`); },
  deleteProperty(_, key) { throw new Error(`state.${String(key)} is read-only: change it through store.js`); },
});

// ---------------------------------------------------------------- event bus

const listeners = new Map();
/** Subscribes to an event; returns the unsubscribe function. */
export function on(event, fn) {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event).add(fn);
  return () => listeners.get(event)?.delete(fn);
}
/** Calls every listener in subscription order; one listener's exception never stops the others. */
export function emit(event, payload) {
  const set = listeners.get(event);
  if (!set) return;
  for (const fn of [...set]) {
    try { fn(payload); } catch (error) { console.error(`[workbench] ${event} listener failed:`, error); }
  }
}

/** Messages for the status bar and the toasts; the status and conflict panels draw them. */
export const notify = {
  status: (message) => emit('status', message),
  hint: (text) => emit('hint', text),
  error: (error, what) => emit('error', { error, what }),
  conflict: (error, others, ops, options) => emit('conflict', { error, others, ops, options }),
};

// ---------------------------------------------------------------- reads

export const shotEntry = (id) => S.model?.shots.get(id) || null;

/** The take and panel the inspector shows for a shot: preview take (UI only) and selected panel. */
export function shotView(shot) {
  let ui = S.shotUi.get(shot.id);
  if (!ui) { ui = { take: null, panel: null }; S.shotUi.set(shot.id, ui); }
  if (!ui.take || !takeOf(shot, ui.take)) ui.take = shot.selected_take;
  const take = takeOf(shot, ui.take) || shot.takes[0] || null;
  if (take) ui.take = take.id;
  let panel = take?.panels.find((candidate) => candidate.id === ui.panel) || take?.panels[0] || null;
  ui.panel = panel?.id || null;
  return { take, panel, ui };
}

// ---------------------------------------------------------------- document

function recompute() {
  setFormat(S.doc.format);
  S.model = buildModel(S.doc);
  S.layout = computeLayout(S.doc, S.model);
  pruneSelection();
}

/** Recomputes indexes and layout from the drawn document and tells every view ('doc'). */
export function refresh(info = {}) {
  if (!S.doc) return;
  recompute();
  emit('doc', { local: true, ...info });
}

function pruneSelection() {
  const model = S.model;
  for (const id of [...S.sel.shots]) if (!model.shots.has(id)) S.sel.shots.delete(id);
  for (const id of [...S.sel.stickies]) if (!model.stickies.has(id)) S.sel.stickies.delete(id);
  if (S.sel.scene && !model.scenes.has(S.sel.scene)) S.sel.scene = null;
  const primary = S.primary;
  if (primary) {
    const alive = primary.type === 'shot' ? model.shots.has(primary.id)
      : primary.type === 'sticky' ? model.stickies.has(primary.id) : model.scenes.has(primary.id);
    if (!alive) S.primary = null;
  }
}

/**
 * Replaces the page's copy with a server document and tells every view ('doc'); a remote change is
 * also announced ('docRemote'). During a drag the document waits (see adoptDeferred). Returns true
 * if the document was drawn.
 */
export function applyServer(doc, version, { self = false, meta = null, initial = false } = {}) {
  normalize(doc);
  freezeDocument(doc);
  S.applyEpoch++;
  if (!initial && !self && version === S.version) return false;
  if (S.dragging) { S.deferred = { doc, version, meta: self ? null : meta }; S.version = version; return false; }
  const old = S.confirmed;
  const oldSignatures = S.model?.signatures || null;
  const previous = S.primary;
  S.confirmed = doc;
  S.doc = doc;
  S.version = version;
  S.remoteName = !self && meta ? eventAuthor(meta.author).name : null;
  recompute();
  if (!self && previous?.type === 'shot' && !S.model.shots.has(previous.id)) S.removedBy = S.remoteName || 'someone else';
  emit('doc', { self, initial, meta, old });
  if (!self && !initial) emit('docRemote', { old, doc, meta, oldSignatures });
  S.remoteName = null;
  return true;
}

/** A remote document that arrived mid-drag becomes the confirmed one now; returns its announcement. */
export function adoptDeferred() {
  const deferred = S.deferred;
  S.deferred = null;
  if (!deferred) return null;
  const old = S.confirmed;
  S.confirmed = deferred.doc;
  S.doc = deferred.doc;
  S.version = deferred.version;
  return () => emit('docRemote', { old, doc: deferred.doc, meta: deferred.meta, oldSignatures: null });
}

/** Draws a local preview of a write (optimisticCommit); revertDraft() returns to the server's copy. */
export function setDraft(doc) { S.doc = freezeDocument(doc); refresh(); }
export function revertDraft() { S.doc = S.confirmed; refresh(); }

// ---------------------------------------------------------------- selection

/** Selects one thing (or nothing). 'beforeSelection' lets a focused field commit first. */
export function selectOnly(type, id, { focus = false } = {}) {
  emit('beforeSelection');
  S.sel.shots.clear(); S.sel.stickies.clear(); S.sel.scene = null;
  if (type === 'shot') S.sel.shots.add(id);
  if (type === 'sticky') S.sel.stickies.add(id);
  if (type === 'scene') S.sel.scene = id;
  S.primary = type ? { type, id } : null;
  S.removedBy = null;
  emit('selection', { type, id, focus });
}
export function toggleSelect(type, id) {
  emit('beforeSelection');
  const set = type === 'shot' ? S.sel.shots : S.sel.stickies;
  if (set.has(id)) {
    set.delete(id);
    if (S.primary?.id === id) {
      const next = [...S.sel.shots][0];
      S.primary = next ? { type: 'shot', id: next } : null;
    }
  } else {
    set.add(id);
    S.primary = { type, id };
  }
  S.sel.scene = null;
  emit('selection', {});
}
export function clearSelection() { selectOnly(null, null); }
/** Changes which selected item the inspector shows, keeping the selection. */
export function setPrimary(type, id) {
  S.primary = type ? { type, id } : null;
  emit('selection', {});
}
/** Replaces the selected shots (marquee). Pass primary to change the inspected shot too. */
export function setShotSelection(shots, ...primary) {
  S.sel.shots = shots;
  if (primary.length) { S.primary = primary[0]; emit('selection', {}); }
  else emit('selection', { classesOnly: true });
}
export function selectAllShots() {
  S.sel.shots = new Set(S.model.order);
  S.sel.stickies.clear();
  if (!S.primary && S.model.order.length) S.primary = { type: 'shot', id: S.model.order[0] };
  emit('selection', {});
}

// ---------------------------------------------------------------- UI state

/** Viewport pan and zoom; the caller redraws (applyView). */
export function setView(partial) { Object.assign(S.view, partial); }
export function setPointer(pointer) { S.pointer = pointer; }
export function setShotFilter(ids) { S.shotFilter = ids; emit('filter', ids); }
export function setDragging(dragging) { S.dragging = dragging; }
export function setViewCut(cutId) { S.viewCut = cutId; }
export function setSequence(sequence) { S.sequence = sequence; }
/** The viewed take and selected panel of a shot (UI only, never written to the project). */
export function setShotUi(shotId, patch) {
  const ui = S.shotUi.get(shotId);
  if (ui) Object.assign(ui, patch);
}
export function toggleShowResolved() { S.showResolved = !S.showResolved; emit('inspector'); }
export function setWorkspace(workspace) { S.workspace = workspace; emit('workspace', workspace); }
export function setLoading(loading) { S.loading = loading; emit('loading'); }
export function setConn(value) { S.conn = value; emit('conn'); }
export function setOffline(offline) {
  S.offline = offline;
  if (offline) S.conn = 'offline';
  emit('conn');
}
export function pushMyCommit(summary) {
  S.myCommits.push(summary);
  if (S.myCommits.length > 100) S.myCommits.shift();
}
export const popMyCommit = () => S.myCommits.pop();
export function bumpSourceEpoch() { S.sourceEpoch++; }

// ---------------------------------------------------------------- project lifecycle

/** Resets per-project state for a project that just loaded (see openProject in api.js). */
export function beginProject(name, result, saved) {
  S.shotUi.clear();
  S.myCommits = [];
  S.sel = { shots: new Set(), stickies: new Set(), scene: null };
  S.primary = null;
  S.removedBy = null;
  S.deferred = null;
  S.project = name;
  S.viewCut = result.sequence?.cut || (saved?.cut && result.document.cuts?.some((c) => c.id === saved.cut) ? saved.cut : null);
  S.sequence = result.sequence;
  S.loading = false;
}
export function endProject() {
  S.project = null; S.doc = null; S.confirmed = null; S.version = null; S.model = null; S.layout = null;
  S.sequence = null; S.viewCut = null; S.shotUi.clear(); S.myCommits = []; S.deferred = null;
  S.primary = null; S.sel = { shots: new Set(), stickies: new Set(), scene: null };
}
