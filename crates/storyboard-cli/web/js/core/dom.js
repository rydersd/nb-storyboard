// DOM helpers shared by every panel, and `els`: the elements each panel creates when it mounts.

export const SVG_NS = 'http://www.w3.org/2000/svg';
export const $ = (selector, root = document) => root.querySelector(selector);

/** Elements by name. A panel adds the elements it creates in its mount(); read them, never replace them. */
export const els = {};

/** Small element builder: h('div', {class: 'x', onclick: fn}, child, 'text'). */
export function h(tag, attrs, ...children) {
  const el = document.createElement(tag);
  if (attrs) {
    for (const [key, value] of Object.entries(attrs)) {
      if (value === undefined || value === null || value === false) continue;
      if (key === 'class') el.className = value;
      else if (key === 'style' && typeof value === 'object') Object.assign(el.style, value);
      else if (key === 'dataset') Object.assign(el.dataset, value);
      else if (key.startsWith('on') && typeof value === 'function') el.addEventListener(key.slice(2), value);
      else if (key === 'text') el.textContent = value;
      else if (key === 'html') el.innerHTML = value;
      else if (value === true) el.setAttribute(key, '');
      else el.setAttribute(key, String(value));
    }
  }
  append(el, children);
  return el;
}
export function append(el, children) {
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    if (Array.isArray(child)) append(el, child);
    else el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
}
export function svg(tag, attrs) {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(attrs || {})) el.setAttribute(key, String(value));
  return el;
}
export const ICON_PLUS = '<svg viewBox="0 0 14 14" aria-hidden="true"><path d="M7 2v10M2 7h10" stroke="currentColor" stroke-width="1.5"/></svg>';
export const ICON_CLOSE = '<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1.5"/></svg>';
export const ICON_COPY = '<svg viewBox="0 0 12 12" aria-hidden="true"><rect x="4" y="4" width="6.5" height="6.5" rx="1" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M8 2.5V2a.5.5 0 0 0-.5-.5h-5A.5.5 0 0 0 2 2v5.5a.5.5 0 0 0 .5.5H3" fill="none" stroke="currentColor" stroke-width="1.2"/></svg>';
export const ICON_EYE_OFF = '<svg viewBox="0 0 14 14" aria-hidden="true"><path d="M1.5 7s2-3.5 5.5-3.5S12.5 7 12.5 7 10.5 10.5 7 10.5 1.5 7 1.5 7z" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M2 12L12 2" stroke="currentColor" stroke-width="1.2"/></svg>';
export function iconButton(label, iconHtml, onclick, extraClass = '') {
  return h('button', { class: `icon-button ${extraClass}`, type: 'button', 'aria-label': label, title: label, html: iconHtml, onclick });
}

export function isTextTarget(target) {
  if (!target || !(target instanceof Element)) return false;
  return target.closest('input, textarea, select, [contenteditable="true"]') !== null;
}

export function autoGrow(el) {
  el.style.height = 'auto';
  el.style.height = `${el.scrollHeight + 2}px`;
}

/** Rebuilds a section without losing the caret of a focused input inside it. */
export function preserveFocus(container, build) {
  const active = document.activeElement;
  let saved = null;
  if (active && container.contains(active) && active.dataset?.key) {
    saved = { key: active.dataset.key, value: active.value, start: active.selectionStart, end: active.selectionEnd };
  }
  build();
  if (!saved) return;
  const el = container.querySelector(`[data-key="${CSS.escape(saved.key)}"]`);
  if (!el) return;
  if (saved.value !== undefined && 'value' in el) el.value = saved.value;
  el.focus({ preventScroll: true });
  try { if (saved.start !== null && saved.start !== undefined) el.setSelectionRange(saved.start, saved.end); } catch { /* not a text input */ }
}
