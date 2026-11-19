// Pure functions over a project document: defaults, indexes, pins, ids, and the local preview of move ops.

import { clamp } from './util.js';

export const STATUSES = ['draft', 'review', 'approved', 'changes'];
export const STATUS_LABEL = { draft: 'Draft', review: 'Review', approved: 'Approved', changes: 'Changes' };
export const KINDS = ['note', 'todo', 'question'];
export const KIND_GLYPH = { note: '¶', todo: '[ ]', question: '?' };
export const KIND_LABEL = { note: 'Note', todo: 'To do', question: 'Question' };

/** Fills the optional fields the server omits when empty, so the rest of the page can rely on them. */
export function normalize(doc) {
  doc.scenes ||= [];
  for (const scene of doc.scenes) {
    scene.shots ||= [];
    for (const shot of scene.shots) {
      shot.takes ||= [];
      for (const take of shot.takes) {
        take.panels ||= [];
        for (const panel of take.panels) panel.layers ||= [];
      }
    }
  }
  doc.cuts ||= [];
  for (const cut of doc.cuts) cut.clips ||= [];
  doc.stickies ||= [];
  doc.layout ||= {};
  doc.layout.positions ||= {};
  doc.format ||= { width: 1920, height: 1080 };
  doc.fps ||= 24;
  return doc;
}

/** Indexes over the drawn document: lookups, story order, pinned sticky counts, per-shot signatures. */
export function buildModel(doc) {
  const shots = new Map();
  const scenes = new Map();
  const order = [];
  doc.scenes.forEach((scene, sceneIndex) => {
    scenes.set(scene.id, { scene, index: sceneIndex });
    scene.shots.forEach((shot, index) => {
      shots.set(shot.id, { shot, scene, sceneIndex, index, seq: `${sceneIndex + 1}.${index + 1}` });
      order.push(shot.id);
    });
  });
  const stickies = new Map();
  const replies = new Map();
  for (const sticky of doc.stickies) {
    stickies.set(sticky.id, sticky);
    if (sticky.reply_to) {
      if (!replies.has(sticky.reply_to)) replies.set(sticky.reply_to, []);
      replies.get(sticky.reply_to).push(sticky);
    }
  }
  const pinnedToShot = new Map();
  const pinnedToScene = new Map();
  for (const sticky of doc.stickies) {
    if (sticky.reply_to) continue;
    const target = pinTarget(sticky.pin);
    if (target.shot) {
      if (!pinnedToShot.has(target.shot)) pinnedToShot.set(target.shot, []);
      pinnedToShot.get(target.shot).push(sticky);
    } else if (target.scene) {
      if (!pinnedToScene.has(target.scene)) pinnedToScene.set(target.scene, []);
      pinnedToScene.get(target.scene).push(sticky);
    }
  }
  const signatures = new Map();
  for (const [id, entry] of shots) {
    signatures.set(id, JSON.stringify([entry.shot, entry.scene.id, entry.index, doc.layout.positions[`shot:${id}`] || null]));
  }
  return { shots, scenes, order, stickies, replies, pinnedToShot, pinnedToScene, signatures };
}

/** "shot:H", "take:H/T", "panel:H/T/P", "layer:H/T/P/L", "scene:S", "clip:C/K", "none". */
export function pinTarget(pin) {
  if (typeof pin !== 'string' || pin === 'none' || !pin) return {};
  const colon = pin.indexOf(':');
  const kind = pin.slice(0, colon);
  const parts = pin.slice(colon + 1).split('/');
  if (kind === 'scene') return { kind, scene: parts[0] };
  if (kind === 'shot' || kind === 'take' || kind === 'panel' || kind === 'layer') {
    return { kind, shot: parts[0], take: parts[1], panel: parts[2], layer: parts[3] };
  }
  return { kind };
}

export const shotStatus = (shot) => (STATUSES.includes(shot.status) ? shot.status : 'draft');
export const takeOf = (shot, takeId) => shot.takes.find((take) => take.id === takeId) || null;

export function nextId(prefix, existing) {
  let max = 0;
  for (const id of existing) {
    const match = new RegExp(`^${prefix}-(\\d+)$`).exec(id);
    if (match) max = Math.max(max, Number(match[1]));
  }
  return `${prefix}-${max + 1}`;
}

/** Local preview of the few ops the page applies optimistically. Returns false if it cannot. */
export function applyLocal(doc, op) {
  const [name, args] = Object.entries(op)[0];
  const positions = doc.layout.positions;
  const findShot = (id) => {
    for (const scene of doc.scenes) {
      const index = scene.shots.findIndex((shot) => shot.id === id);
      if (index >= 0) return { scene, index };
    }
    return null;
  };
  if (name === 'move_node') {
    if (args.position) positions[args.node] = { x: args.position.x, y: args.position.y };
    else delete positions[args.node];
    return true;
  }
  if (name === 'move_shot' || name === 'move_shot_between_scenes') {
    const found = findShot(args.shot);
    if (!found) return false;
    const [shot] = found.scene.shots.splice(found.index, 1);
    const target = name === 'move_shot' ? found.scene : doc.scenes.find((scene) => scene.id === args.scene);
    if (!target) return false;
    target.shots.splice(clamp(args.index, 0, target.shots.length), 0, shot);
    return true;
  }
  if (name === 'batch') return args.ops.every((inner) => applyLocal(doc, inner));
  return false;
}
