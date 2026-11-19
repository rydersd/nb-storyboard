// Full-size review of the selected panel over the graph: Left and Right step, Esc returns.

import { panelUrl } from '../core/api.js';
import { addKeyFilter } from '../core/commands.js';
import { els, h } from '../core/dom.js';
import { registerPanel } from '../core/registry.js';
import { shotEntry, shotView, state } from '../core/store.js';
import { clamp } from '../core/util.js';
import { stepPanel } from './inspector/sections.js';

export function openReview() {
  const entry = state.primary?.type === 'shot' && shotEntry(state.primary.id);
  if (!entry) return;
  const { take, panel } = shotView(entry.shot);
  if (!take || !panel) return;
  els.review.hidden = false;
  const width = clamp(Math.round(els.graph.clientWidth * (window.devicePixelRatio || 1)), 16, 4096);
  els.reviewImg.src = panelUrl(entry.shot.id, take.id, panel, width);
  els.reviewImg.alt = `Panel ${take.panels.indexOf(panel) + 1} of shot ${entry.seq}`;
  els.reviewCaption.textContent = `${entry.seq} ${entry.shot.title || 'Untitled'}, panel ${take.panels.indexOf(panel) + 1} of ${take.panels.length}. Left and Right step, Esc returns.`;
}
export function closeReview() { els.review.hidden = true; els.reviewImg.removeAttribute('src'); }

export const isReviewOpen = () => !!els.review && !els.review.hidden;

registerPanel({
  id: 'review', title: 'Panel review', region: 'graph', order: 20,
  mount(el, ctx) {
    els.reviewImg = h('img', { id: 'review-img', alt: '' });
    els.reviewCaption = h('div', { id: 'review-caption', class: 'review-caption' });
    els.review = h('div', { id: 'review', class: 'review', hidden: true, role: 'dialog', 'aria-label': 'Panel review' }, els.reviewImg, els.reviewCaption);
    el.append(els.review);
    ctx.store.on('review', () => openReview());
    ctx.store.on('beforeProjectChange', () => closeReview());
    return {};
  },
});

addKeyFilter((event) => {
  if (!isReviewOpen()) return false;
  const key = event.key;
  if (key === 'Escape') { event.preventDefault(); closeReview(); }
  else if (key === 'ArrowLeft' || key === 'ArrowRight') { event.preventDefault(); stepPanel(key === 'ArrowLeft' ? -1 : 1); openReview(); }
  return true;
});
