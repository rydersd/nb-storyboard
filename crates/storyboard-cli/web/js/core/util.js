// Formatting, authors, hashing, timing and browser-local preferences. No DOM, no app state.

import { h } from './dom.js';

const STORE_PREFIX = 'nb-workbench:';
export const enc = encodeURIComponent;
export const clamp = (value, lo, hi) => Math.min(hi, Math.max(lo, value));

export function fmtSeconds(seconds) {
  const value = Number(seconds) || 0;
  return `${Math.round(value * 100) / 100}s`;
}
export function fmtDuration(frames, fps) {
  const seconds = frames / (fps || 24);
  if (seconds < 60) return `${Math.round(seconds * 10) / 10} s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(Math.round(seconds % 60)).padStart(2, '0')}`;
}
/** A compact timeline clock for project-list summaries. */
export function fmtClock(frames, fps) {
  const seconds = Math.max(0, Math.round((Number(frames) || 0) / (Number(fps) || 24)));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
export function relTime(iso) {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return '';
  const seconds = Math.max(0, Math.round((Date.now() - then) / 1000));
  if (seconds < 5) return 'just now';
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return new Date(then).toLocaleDateString();
}
/** RFC 3339 UTC, without milliseconds. */
export const nowIso = () => new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');

/** A stored sticky author: "human" or {agent: {name}}. */
export function stickyAuthor(author) {
  if (author && typeof author === 'object' && author.agent) return { kind: 'agent', name: author.agent.name || 'agent' };
  return { kind: 'human', name: 'You' };
}
/** An event or history author: "human", "agent:NAME", {agent:{name}} or null (a hand edit). */
export function eventAuthor(author) {
  if (author && typeof author === 'object') return stickyAuthor(author);
  if (typeof author === 'string' && author.startsWith('agent:')) return { kind: 'agent', name: author.slice(6) };
  if (author === 'human') return { kind: 'human', name: 'You' };
  return { kind: 'agent', name: 'File edit' };
}

export function fnv(text) {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index++) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(36);
}

/** Browser-local preferences (view, last project, inspector width). Failures are ignored. */
export const prefs = {
  get(key) {
    try { const raw = localStorage.getItem(STORE_PREFIX + key); return raw === null ? null : JSON.parse(raw); } catch { return null; }
  },
  set(key, value) {
    try { localStorage.setItem(STORE_PREFIX + key, JSON.stringify(value)); } catch { /* storage unavailable: nothing to keep */ }
  },
};

export function debounce(fn, ms) {
  let timer = 0;
  const wrapped = (...args) => { clearTimeout(timer); timer = setTimeout(() => fn(...args), ms); };
  wrapped.flush = (...args) => { clearTimeout(timer); fn(...args); };
  wrapped.cancel = () => clearTimeout(timer);
  return wrapped;
}
