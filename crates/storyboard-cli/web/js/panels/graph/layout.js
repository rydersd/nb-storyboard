// Graph geometry: node and frame sizes, auto-layout (UX 2.6), bounds, story-line wires and hit tests.

import { pinTarget } from '../../core/model.js';
import { state } from '../../core/store.js';
import { clamp } from '../../core/util.js';

export const NODE_W = 176;
export const NODE_TEXT_H = 46;
export const SLOT = NODE_W + 40;
export const FRAME_PAD = 16;
export const FRAME_HEADER = 28;
export const ROW_GAP = 64;
export const GHOST = 40;
export const STICKY_W = 200;
export const PILL_W = 140;
export const PILL_H = 36;
export const HIT_PX = 16;

export let NODE_THUMB_H = 99;
export let NODE_H = NODE_THUMB_H + NODE_TEXT_H + 2;

export function setFormat(format) {
  const ratio = format && format.width > 0 ? format.height / format.width : 9 / 16;
  NODE_THUMB_H = Math.round(NODE_W * ratio);
  NODE_H = NODE_THUMB_H + NODE_TEXT_H + 2;
}

/**
 * Auto-layout (UX 2.6): scenes are rows in story order, shots run left to right; a stored position
 * wins for its own shot only. Frames are derived from their shots, never stored.
 */
export function computeLayout(doc, model) {
  const positions = doc.layout.positions;
  const shots = new Map();
  const frames = new Map();
  let rowY = 0;
  let widest = 0;
  const rowHeight = FRAME_HEADER + FRAME_PAD + NODE_H + FRAME_PAD;
  for (const scene of doc.scenes) {
    const autoY = rowY + FRAME_HEADER + FRAME_PAD;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    scene.shots.forEach((shot, index) => {
      const stored = positions[`shot:${shot.id}`];
      const x = stored ? stored.x : FRAME_PAD + index * SLOT;
      const y = stored ? stored.y : autoY;
      shots.set(shot.id, { x, y, stored: !!stored });
      minX = Math.min(minX, x); minY = Math.min(minY, y);
      maxX = Math.max(maxX, x + NODE_W); maxY = Math.max(maxY, y + NODE_H);
    });
    let ghost;
    if (scene.shots.length) {
      const last = shots.get(scene.shots[scene.shots.length - 1].id);
      ghost = { x: last.x + NODE_W + 16, y: last.y + NODE_H / 2 - GHOST / 2 };
      maxX = Math.max(maxX, ghost.x + GHOST);
    } else {
      minX = FRAME_PAD; minY = autoY; maxX = FRAME_PAD + 220; maxY = autoY + 56;
      ghost = { x: FRAME_PAD, y: autoY + 8 };
    }
    const frame = {
      x: minX - FRAME_PAD, y: minY - FRAME_PAD - FRAME_HEADER,
      w: maxX - minX + FRAME_PAD * 2, h: maxY - minY + FRAME_PAD * 2 + FRAME_HEADER, ghost, empty: !scene.shots.length,
    };
    frames.set(scene.id, frame);
    widest = Math.max(widest, frame.x + frame.w);
    rowY += (scene.shots.length ? rowHeight : FRAME_HEADER + FRAME_PAD * 2 + 56) + ROW_GAP;
  }
  // free stickies: stored position, or a column 80 px right of the widest row
  const stickies = new Map();
  let stackY = 0;
  for (const sticky of doc.stickies) {
    if (sticky.reply_to || sticky.resolved || pinTarget(sticky.pin).kind) continue;
    const stored = positions[`sticky:${sticky.id}`];
    if (stored) stickies.set(sticky.id, { x: stored.x, y: stored.y, stored: true });
    else { stickies.set(sticky.id, { x: widest + 80, y: stackY, stored: false }); stackY += 132; }
  }
  return { shots, frames, stickies, widest, height: rowY };
}

/** Bounding box of everything drawn, in world coordinates. */
export function worldBounds(ids) {
  const layout = state.layout;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  const add = (x, y, w, hgt) => { minX = Math.min(minX, x); minY = Math.min(minY, y); maxX = Math.max(maxX, x + w); maxY = Math.max(maxY, y + hgt); };
  if (ids) {
    for (const id of ids) {
      const p = layout.shots.get(id);
      if (p) add(p.x, p.y, NODE_W, NODE_H);
      const s = layout.stickies.get(id);
      if (s) add(s.x, s.y, STICKY_W, 110);
    }
  } else {
    for (const frame of layout.frames.values()) add(frame.x, frame.y, frame.w, frame.h);
    for (const s of layout.stickies.values()) add(s.x, s.y, STICKY_W, 110);
  }
  return minX === Infinity ? null : { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}

// ---------------------------------------------------------------- wires

export const portOut = (p, w = NODE_W, hgt = NODE_H) => ({ x: p.x + w, y: p.y + hgt / 2 });
export const portIn = (p, hgt = NODE_H) => ({ x: p.x, y: p.y + hgt / 2 });

/** Story-line geometry from a to b: an S-curve forward, or a row-return route back to the left. */
export function wireSegments(a, b) {
  if (b.x >= a.x + 24) {
    const dx = Math.max(30, (b.x - a.x) / 2);
    return [{ c: [a, { x: a.x + dx, y: a.y }, { x: b.x - dx, y: b.y }, b] }];
  }
  const r = 48;
  let ym;
  if (Math.abs(b.y - a.y) > NODE_H + 40) ym = (a.y + b.y) / 2;
  else ym = Math.max(a.y, b.y) + NODE_H / 2 + 40;
  return [
    { c: [a, { x: a.x + r, y: a.y }, { x: a.x + r, y: ym }, { x: a.x + 8, y: ym }] },
    { l: [{ x: a.x + 8, y: ym }, { x: b.x - 8, y: ym }] },
    { c: [{ x: b.x - 8, y: ym }, { x: b.x - r, y: ym }, { x: b.x - r, y: b.y }, b] },
  ];
}
export function segmentsPath(segments) {
  let d = '';
  segments.forEach((segment, index) => {
    if (segment.c) {
      const [p0, p1, p2, p3] = segment.c;
      d += `${index === 0 ? `M${p0.x} ${p0.y}` : ''}C${p1.x} ${p1.y} ${p2.x} ${p2.y} ${p3.x} ${p3.y}`;
    } else {
      const [p0, p1] = segment.l;
      d += `${index === 0 ? `M${p0.x} ${p0.y}` : ''}L${p1.x} ${p1.y}`;
    }
  });
  return d;
}
export function sampleSegments(segments) {
  const points = [];
  for (const segment of segments) {
    if (segment.c) {
      const [p0, p1, p2, p3] = segment.c;
      for (let step = points.length ? 1 : 0; step <= 14; step++) {
        const t = step / 14, u = 1 - t;
        points.push({
          x: u * u * u * p0.x + 3 * u * u * t * p1.x + 3 * u * t * t * p2.x + t * t * t * p3.x,
          y: u * u * u * p0.y + 3 * u * u * t * p1.y + 3 * u * t * t * p2.y + t * t * t * p3.y,
        });
      }
    } else {
      const [p0, p1] = segment.l;
      for (let step = points.length ? 1 : 0; step <= 8; step++) {
        points.push({ x: p0.x + (p1.x - p0.x) * step / 8, y: p0.y + (p1.y - p0.y) * step / 8 });
      }
    }
  }
  const lengths = [0];
  for (let index = 1; index < points.length; index++) {
    lengths.push(lengths[index - 1] + Math.hypot(points[index].x - points[index - 1].x, points[index].y - points[index - 1].y));
  }
  return { points, lengths };
}
/** Nearest point on a sampled polyline: distance, the point, its tangent and its fraction of the length. */
export function nearestOnPolyline(sample, p) {
  const { points, lengths } = sample;
  let best = { d: Infinity };
  for (let index = 1; index < points.length; index++) {
    const a = points[index - 1], b = points[index];
    const vx = b.x - a.x, vy = b.y - a.y;
    const len2 = vx * vx + vy * vy || 1;
    const t = clamp(((p.x - a.x) * vx + (p.y - a.y) * vy) / len2, 0, 1);
    const x = a.x + vx * t, y = a.y + vy * t;
    const d = Math.hypot(p.x - x, p.y - y);
    if (d < best.d) {
      const len = Math.sqrt(len2);
      best = { d, x, y, tx: vx / len, ty: vy / len, at: (lengths[index - 1] + len * t) / (lengths[lengths.length - 1] || 1) };
    }
  }
  return best;
}
