// Live updates: the event stream, reconnecting and going offline, and naming what changed remotely
// ('remote' carries the changed shots and stickies so the graph can highlight them).

import { closeProject, openProject, refreshProject, refreshWorkspace, refreshWorkspaceSoon } from './api.js';
import { buildModel, pinTarget } from './model.js';
import { bumpSourceEpoch, emit, notify, on, refresh, setConn, setOffline, state } from './store.js';
import { debounce, eventAuthor, nowIso } from './util.js';

export const OFFLINE_AFTER_MS = 10000;

/** UX 5: ring and author chip on each changed node; status bar names the change; no focus moves. */
export function announceRemote(old, doc, meta, oldSignatures) {
  if (!old || !meta) return;
  const who = eventAuthor(meta.author);
  const signatures = oldSignatures || buildModel(old).signatures;
  const changed = [];
  for (const [id, signature] of state.model.signatures) if (signatures.get(id) !== signature) changed.push(id);
  const before = new Map(old.stickies.map((s) => [s.id, JSON.stringify(s)]));
  const changedStickies = [];
  for (const sticky of doc.stickies) {
    if (before.get(sticky.id) === JSON.stringify(sticky)) continue;
    const root = sticky.reply_to ? state.model.stickies.get(sticky.reply_to) || sticky : sticky;
    changedStickies.push(root.id);
    const target = pinTarget(root.pin);
    if (target.shot && !changed.includes(target.shot)) changed.push(target.shot);
  }
  emit('remote', { who, shots: changed, stickies: changedStickies });
  notify.status({ who, text: meta.summary || 'changed the project', at: nowIso(), ids: [...changed, ...changedStickies] });
}

let events = null;
let offlineTimer = 0;
let lostConnection = false;

export function connectEvents() {
  events?.close();
  events = new EventSource('/api/events');
  events.addEventListener('open', () => {
    clearTimeout(offlineTimer);
    offlineTimer = 0;
    const wasLost = lostConnection;
    lostConnection = false;
    setOffline(false);
    setConn('live');
    if (wasLost) {
      refreshWorkspace();
      refreshProject();
      emit('reconnected');
    }
  });
  events.addEventListener('error', () => {
    lostConnection = true;
    if (!state.offline) setConn('reconnecting');
    if (!offlineTimer && !state.offline) offlineTimer = setTimeout(() => { offlineTimer = 0; setOffline(true); }, OFFLINE_AFTER_MS);
    if (events.readyState === EventSource.CLOSED) setTimeout(connectEvents, 2000);
  });
  events.addEventListener('project', (event) => {
    let data;
    try { data = JSON.parse(event.data); } catch { return; }
    refreshWorkspaceSoon();
    if (!state.project || data.project !== state.project || data.version === state.version) return;
    refreshProject(data.author === 'human' ? null : { author: data.author, summary: data.summary });
  });
  events.addEventListener('workspace', async () => {
    const workspace = await refreshWorkspace();
    if (!workspace) return;
    if (state.project && !workspace.projects.some((p) => p.project === state.project)) {
      notify.status({ text: `${state.project} is no longer in the workspace.`, error: true });
      closeProject();
    }
  });
  events.addEventListener('source', (event) => {
    let data;
    try { data = JSON.parse(event.data); } catch { return; }
    if (data.project === state.project) sourcesChanged();
  });
}
export const sourcesChanged = debounce(() => { bumpSourceEpoch(); emit('sources'); refresh(); }, 300);

/** Starts announcing remote changes. connectEvents() opens the stream. */
export function startLive() {
  on('docRemote', ({ old, doc, meta, oldSignatures }) => announceRemote(old, doc, meta, oldSignatures));
}
