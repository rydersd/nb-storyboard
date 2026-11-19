// HTTP: fetch wrappers and the error shape, ordered writes (commit, undo), project loading.
// Contract: wiki/product-spec/plans/workbench-api.md.

import { applyLocal } from './model.js';
import { insertionAfterShot } from './import-shot.js';
import { applyServer, beginProject, emit, endProject, notify, popMyCommit, pushMyCommit, revertDraft, setDraft, setLoading, setSequence, setViewCut, setWorkspace, state } from './store.js';
import { debounce, enc, eventAuthor, fnv, nowIso, prefs } from './util.js';

export class ApiFailure extends Error {
  constructor(status, error) {
    super(error?.message || `request failed (${status})`);
    this.status = status;
    this.kind = error?.kind || 'http';
    this.hint = error?.hint || '';
    this.valid = Array.isArray(error?.valid) ? error.valid : [];
    this.issues = Array.isArray(error?.issues) ? error.issues : [];
  }
  get intervening() { return this.issues[0]?.intervening || []; }
  get currentVersion() { return this.issues[0]?.current_version || null; }
}

export async function api(path, options) {
  let response;
  try {
    response = await fetch(path, options);
  } catch (error) {
    throw new ApiFailure(0, { kind: 'network', message: 'Could not reach storyboard.', hint: 'Check that storyboard serve is still running.' });
  }
  let body = null;
  try { body = await response.json(); } catch { body = null; }
  if (!response.ok || !body || body.error) {
    throw new ApiFailure(response.status, body?.error || { kind: 'http', message: `${response.status} ${response.statusText}` });
  }
  return body;
}
export const getWorkspace = () => api('/api/workspace');
export const getProject = (project, cut) => api(`/api/project?project=${enc(project)}${cut ? `&cut=${enc(cut)}` : ''}`);
export const getHistory = (project, limit = 80) => api(`/api/history?project=${enc(project)}&limit=${limit}`);
export const getSources = (project) => api(`/api/sources?project=${enc(project)}`);
export const getViewport = (project) => api(`/api/viewport?project=${enc(project)}`);
export const saveViewport = (project, viewport) => postJson('/api/viewport', { project, viewport });
export const getLayout = () => api('/api/layout');
export const saveLayout = (layout) => postJson('/api/layout', { layout });
export const postJson = (path, body) => api(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
let projectChangeSerial = 0;
export async function createProject(details) {
  const request = ++projectChangeSerial;
  if (!await prepareProjectChange()) {
    if (request === projectChangeSerial) setLoading(false);
    throw new Error(activeUploads ? 'Wait for the current upload before creating a project.' : 'Resolve the current edit before creating a project.');
  }
  if (request !== projectChangeSerial) throw new Error('Another project change was requested before this project was created.');
  const result = await postJson('/api/new-project', details);
  await refreshWorkspace();
  if (request !== projectChangeSerial) throw new Error(`Created ${result.project}, but another project change was requested. Select the new project to open it.`);
  if (!await openProjectForRequest(result.project, request)) throw new Error(`Created ${result.project}, but could not open it. Resolve the current edit, then select the new project.`);
  return result;
}

let activeUploads = 0;
async function upload(path, file) {
  activeUploads++;
  try {
    return await api(path, { method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body: file });
  } finally {
    activeUploads--;
  }
}
export const uploadShotImage = (project, folder, file) => upload(
  `/api/upload-source?project=${enc(project)}&folder=${enc(folder)}&name=${enc(file.name)}`,
  file,
);
export const uploadMovie = (project, folder, file) => upload(
  `/api/upload-movie?project=${enc(project)}&folder=${enc(folder)}`,
  file,
);
export const uploadAudio = (project, folder, ext, file) => upload(
  `/api/upload-audio?project=${enc(project)}&folder=${enc(folder)}&ext=${enc(ext)}`,
  file,
);

let writeChain = Promise.resolve();
async function settleWrites() {
  let pending;
  do { pending = writeChain; await pending; } while (pending !== writeChain);
}
async function prepareProjectChange() {
  if (activeUploads) {
    notify.status({ text: 'Wait for the current upload before changing projects.', error: true });
    return false;
  }
  if (state.offline && state.project) {
    notify.status({ text: 'Reconnecting: cannot change projects with unsaved edits.', error: true });
    return false;
  }
  emit('beforeProjectChange');
  return readyToDropProject();
}
async function readyToDropProject() {
  await settleWrites();
  if (activeUploads) {
    notify.status({ text: 'Wait for the current upload before changing projects.', error: true });
    return false;
  }
  if (state.offline && state.project) {
    notify.status({ text: 'Reconnecting: cannot change projects with unsaved edits.', error: true });
    return false;
  }
  if (document.querySelector('.field-error:not([hidden]), .conflict-bar:not([hidden])')) {
    notify.status({ text: 'Resolve the inspector field error before changing projects.', error: true });
    return false;
  }
  return true;
}

/** Commit an uploaded image sequence in order with other local writes. */
export function importShot(project, details) {
  const run = async () => {
    if (state.project !== project) throw new Error('The project changed during import.');
    try {
      const { afterShot, ...request } = details;
      const index = insertionAfterShot(state.doc, request.scene, afterShot);
      const result = await postJson('/api/import-shot', {
        project, base_version: state.version, ...request, index,
      });
      if (state.project !== project) return null;
      pushMyCommit(result.summary);
      if (applyServer(result.document, result.version, { self: true }) && state.viewCut) refreshSequenceSoon();
      notify.status({ who: eventAuthor('human'), text: result.summary, at: nowIso(), ids: result.ids });
      return result;
    } catch (error) {
      if (error instanceof ApiFailure && error.kind === 'conflict') await refreshProject();
      throw error;
    }
  };
  const promise = writeChain.then(run, run);
  writeChain = promise.catch(() => null);
  return promise;
}

export function importMovie(project, details) {
  const run = async () => {
    if (state.project !== project) throw new Error('The project changed during movie import.');
    const result = await postJson('/api/import-movie', { project, base_version: state.version, ...details });
    if (state.project !== project) return null;
    pushMyCommit(result.summary);
    if (applyServer(result.document, result.version, { self: true }) && state.viewCut) refreshSequenceSoon();
    notify.status({ text: result.summary, at: nowIso(), ids: result.ids });
    return result;
  };
  const promise = writeChain.then(run, run);
  writeChain = promise.catch(() => null);
  return promise;
}

export function splitMovieShot(project, details) {
  const run = async () => {
    if (state.project !== project) throw new Error('The project changed before the split.');
    try {
      const result = await postJson('/api/split-movie-shot', { project, base_version: state.version, ...details });
      if (state.project !== project) return null;
      pushMyCommit(result.summary);
      if (applyServer(result.document, result.version, { self: true }) && state.viewCut) refreshSequenceSoon();
      notify.status({ who: eventAuthor('human'), text: result.summary, at: nowIso(), ids: result.ids });
      return result;
    } catch (error) {
      if (error instanceof ApiFailure && error.kind === 'conflict') await refreshProject();
      throw error;
    }
  };
  const promise = writeChain.then(run, run);
  writeChain = promise.catch(() => null);
  return promise;
}

export function moveMovieBoundary(project, details, expectedVersion) {
  const run = async () => {
    if (state.project !== project || state.version !== expectedVersion) {
      throw new Error('The project changed before the boundary edit. Refresh and retry.');
    }
    try {
      const result = await postJson('/api/move-movie-boundary', { project, base_version: expectedVersion, ...details });
      if (state.project !== project) return null;
      pushMyCommit(result.summary);
      if (applyServer(result.document, result.version, { self: true }) && state.viewCut) refreshSequenceSoon();
      notify.status({ who: eventAuthor('human'), text: result.summary, at: nowIso(), ids: result.ids });
      return result;
    } catch (error) {
      if (error instanceof ApiFailure && error.kind === 'conflict') await refreshProject();
      throw error;
    }
  };
  const promise = writeChain.then(run, run);
  writeChain = promise.catch(() => null);
  return promise;
}

/**
 * Commits ops through the server, one at a time and in order. `base` is the version the person's
 * edit started from (focus or drag start); a 409 caused only by my own earlier commits re-applies
 * silently, any other 409 goes to `onConflict` or the conflict bar.
 */
export function commit(ops, options = {}) {
  if (!ops.length) return Promise.resolve(null);
  if (state.offline) {
    notify.status({ text: 'Offline: edits are paused until storyboard is back.', error: true });
    options.onOffline?.();
    return Promise.resolve(null);
  }
  const project = state.project;
  const body = ops.length === 1 ? ops : [{ batch: { ops } }];
  const run = async () => {
    if (state.project !== project) return null;
    let base = options.base || state.version;
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        const result = await postJson('/api/commit', { project, base_version: base, ops: body });
        if (state.project !== project) return null;
        pushMyCommit(result.summary);
        if (applyServer(result.document, result.version, { self: true }) && state.viewCut) refreshSequenceSoon();
        if (!options.quiet) notify.status({ who: eventAuthor('human'), text: options.label || result.summary, at: nowIso(), ids: result.ids });
        options.onDone?.(result);
        return result;
      } catch (error) {
        if (!(error instanceof ApiFailure)) throw error;
        if (error.kind === 'conflict') {
          const fromOthers = error.intervening.filter((record) => eventAuthor(record.author).kind !== 'human');
          if (!options.strictBase && fromOthers.length === 0 && attempt === 0 && error.intervening.length > 0) {
            base = error.currentVersion || state.version;
            continue;
          }
          await refreshProject();
          if (options.onConflict) options.onConflict(error, fromOthers);
          else notify.conflict(error, fromOthers, ops, options);
        } else {
          if (options.onError) options.onError(error);
          notify.error(error, options.what);
        }
        options.onFail?.(error);
        return null;
      }
    }
    return null;
  };
  const promise = writeChain.then(run, run);
  writeChain = promise.catch(() => null);
  return promise;
}

/** Commits with an immediate local preview of move ops; the server's document replaces it either way. */
export function optimisticCommit(ops, options = {}) {
  const draft = structuredClone(state.doc);
  let applied = true;
  for (const op of ops) applied = applyLocal(draft, op) && applied;
  if (applied) setDraft(draft);
  return commit(ops, {
    ...options,
    onFail: (error) => {
      revertDraft();
      options.onFail?.(error);
    },
  });
}

export async function undo() {
  if (!state.project) return;
  if (state.offline) { notify.status({ text: 'Offline: undo is paused.', error: true }); return; }
  const project = state.project;
  const run = async () => {
    try {
      const result = await postJson('/api/undo', { project, base_version: state.version });
      if (state.project !== project) return;
      const named = popMyCommit();
      if (applyServer(result.document, result.version, { self: true }) && state.viewCut) refreshSequenceSoon();
      notify.status({ who: eventAuthor('human'), text: named ? `undid ${named}` : result.summary, at: nowIso() });
    } catch (error) {
      if (!(error instanceof ApiFailure)) throw error;
      if (error.kind === 'not_found') notify.status({ text: 'Nothing of yours to undo.' });
      else if (error.kind === 'conflict') {
        await refreshProject();
        notify.conflict(error, error.intervening.filter((r) => eventAuthor(r.author).kind !== 'human'), null, {});
      } else notify.error(error, 'Undo');
    }
  };
  const promise = writeChain.then(run, run);
  writeChain = promise.catch(() => null);
  return promise;
}

/** Reapply the person's most recently undone commit through the same serialized write path as undo. */
export async function redo() {
  if (!state.project) return;
  if (state.offline) { notify.status({ text: 'Offline: redo is paused.', error: true }); return; }
  const project = state.project;
  const run = async () => {
    try {
      const result = await postJson('/api/redo', { project, base_version: state.version });
      if (state.project !== project) return;
      if (applyServer(result.document, result.version, { self: true }) && state.viewCut) refreshSequenceSoon();
      notify.status({ who: eventAuthor('human'), text: result.summary, at: nowIso() });
    } catch (error) {
      if (!(error instanceof ApiFailure)) throw error;
      if (error.kind === 'not_found') notify.status({ text: 'Nothing of yours to redo.' });
      else if (error.kind === 'conflict') { await refreshProject(); notify.conflict(error, error.intervening.filter((r) => eventAuthor(r.author).kind !== 'human'), null, {}); }
      else notify.error(error, 'Redo');
    }
  };
  const promise = writeChain.then(run, run);
  writeChain = promise.catch(() => null);
  return promise;
}

let refreshInFlight = null;
let refreshAgain = false;
let pendingMeta = null;
/** Refetches the open project (and the viewed cut's sequence). Stale answers are dropped. */
export async function refreshProject(meta = null) {
  if (!state.project) return;
  const project = state.project;
  if (pendingMeta?.project !== project) pendingMeta = null;
  if (meta) pendingMeta = { project, value: meta };
  if (refreshInFlight) {
    const current = refreshInFlight;
    refreshAgain = true;
    await current;
    if (state.project === project && refreshInFlight && refreshInFlight !== current) await refreshInFlight;
    return;
  }
  const epoch = state.applyEpoch;
  const cut = state.viewCut;
  refreshInFlight = (async () => {
    try {
      const result = await getProject(project, cut);
      if (state.project !== project) return;
      if (state.viewCut !== cut) { refreshAgain = true; return; }
      if (state.applyEpoch !== epoch) {
        if (result.version !== state.version) refreshAgain = true;
        return;
      }
      setSequence(result.sequence);
      const pending = pendingMeta?.project === project ? pendingMeta.value : null;
      if (pendingMeta?.project === project) pendingMeta = null;
      applyServer(result.document, result.version, { self: false, meta: pending });
    } catch (error) {
      if (state.project === project && state.viewCut !== cut) refreshAgain = true;
      else if (state.project === project && error instanceof ApiFailure && error.kind !== 'network') {
        if (state.viewCut && /cut/i.test(error.message)) { setViewCut(null); refreshAgain = true; }
        else notify.error(error, 'Loading the project');
      }
    } finally {
      if (state.project !== project && pendingMeta?.project === project) pendingMeta = null;
      refreshInFlight = null;
      if (refreshAgain) { refreshAgain = false; refreshProject(); }
    }
  })();
  return refreshInFlight;
}

export const refreshSequenceSoon = debounce(async () => {
  if (!state.project || !state.viewCut) return;
  const project = state.project;
  const cut = state.viewCut;
  const version = state.version;
  try {
    const result = await getProject(project, cut);
    if (state.project === project && state.viewCut === cut && state.version === version && result.version === version) {
      setSequence(result.sequence); emit('inspector');
    }
  } catch { /* the next event refreshes it */ }
}, 250);

export function panelUrl(shotId, takeId, panel, width, mode = 'stage') {
  const doc = state.doc;
  const key = fnv(JSON.stringify(panel) + JSON.stringify(doc.format) + state.sourceEpoch);
  return `/api/panel.png?project=${enc(state.project)}&shot=${enc(shotId)}&take=${enc(takeId)}&panel=${enc(panel.id)}&width=${Math.round(width)}&mode=${mode}&k=${key}`;
}

export async function refreshWorkspace() {
  try {
    setWorkspace(await getWorkspace());
    return state.workspace;
  } catch (error) {
    if (error instanceof ApiFailure && error.kind !== 'network') notify.error(error, 'Reading the workspace');
    return null;
  }
}
export const refreshWorkspaceSoon = debounce(refreshWorkspace, 400);

/**
 * Opens a project. 'beforeProjectChange' lets views finish edits; 'projectDrop' drops the old
 * project from the page entirely; 'projectOpened' restores its view.
 */
export function openProject(name) { return openProjectForRequest(name, ++projectChangeSerial); }
async function openProjectForRequest(name, request) {
  if (request !== projectChangeSerial) return null;
  if (!await prepareProjectChange()) { if (request === projectChangeSerial) setLoading(false); return null; }
  if (request !== projectChangeSerial) return null;
  // The project cache is authoritative across browsers. Local preferences are only a route-failure fallback.
  let saved;
  try {
    const cached = await getViewport(name);
    if (cached.viewport) {
      saved = { x: cached.viewport.pan_x, y: cached.viewport.pan_y, z: cached.viewport.zoom, cut: cached.viewport.viewed_cut };
    }
  } catch {
    saved = prefs.get(`view:${name}`);
  }
  setLoading(true);
  let result;
  try {
    result = await getProject(name, saved?.cut || null);
  } catch (error) {
    if (request !== projectChangeSerial) return null;
    if (saved?.cut) { try { result = await getProject(name, null); } catch { result = null; } }
    if (request !== projectChangeSerial) return null;
    if (!result) {
      setLoading(false);
      if (error instanceof ApiFailure) notify.error(error, 'Opening the project');
      return;
    }
  }
  if (request !== projectChangeSerial) return null;
  // The project read can overlap a new upload or edit in the project being left.
  if (!await readyToDropProject()) { if (request === projectChangeSerial) setLoading(false); return null; }
  if (request !== projectChangeSerial) return null;
  emit('projectDrop');
  beginProject(name, result, saved);
  prefs.set('last-project', name);
  applyServer(result.document, result.version, { self: true, initial: true });
  emit('projectOpened', { saved });
  notify.status({ text: `Opened ${result.document.title || name}.` });
  return result;
}

export async function closeProject() {
  const request = ++projectChangeSerial;
  if (!state.project) { setLoading(false); return true; }
  if (!await prepareProjectChange()) { if (request === projectChangeSerial) setLoading(false); return false; }
  if (request !== projectChangeSerial) return false;
  emit('projectDrop');
  endProject();
  setLoading(false);
  prefs.set('last-project', false);
  emit('projectClosed');
  notify.status({ text: 'Project closed.' });
  return true;
}
