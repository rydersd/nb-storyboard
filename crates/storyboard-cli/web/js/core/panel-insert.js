// Insert a board panel at a visible frame and keep the strip in playback order.

import { contentDuration } from './timing.js';

export function panelInsertionOps(shot, take, index, fps) {
  if (!Number.isInteger(index) || index < 0 || index > take.panels.length) throw new Error('Choose a panel position within the take.');
  const frameOf = (time) => Math.ceil(time * fps - 1e-8);
  const frame = index ? frameOf(take.panels[index - 1].time) + 1 : 0;
  if (frame >= 3600 * fps) throw new Error('The shot cannot extend beyond one hour.');
  const ops = [];
  let nextFrame = frame + 1;
  for (const panel of take.panels.slice(index)) {
    const current = frameOf(panel.time);
    if (current < nextFrame) {
      ops.push({ update_panel: { shot: shot.id, take: take.id, panel: panel.id,
        patch: { time: nextFrame / fps } } });
      nextFrame++;
    } else {
      nextFrame = current + 1;
    }
  }
  ops.push({ add_panel: { shot: shot.id, take: take.id, index, time: frame / fps } });
  const lastFrame = Math.max(frame, nextFrame - 1);
  if (lastFrame / fps >= shot.duration - 1e-8) {
    const duration = (lastFrame + 1) / fps;
    if (duration > 3600) throw new Error('The shot cannot extend beyond one hour.');
    // Undo runs inverse operations in reverse order. Restore the old duration only after
    // the inserted or shifted panels have been removed from its end.
    ops.unshift({ update_shot: { shot: shot.id, patch: { duration } } });
  }
  return ops;
}

export function panelRetimeOps(shot, take, panelId, seconds, fps) {
  if (!Number.isFinite(seconds) || seconds < 0) throw new Error('Enter seconds from the shot start, 0 or more.');
  const frame = Math.round(seconds * fps);
  if (frame >= 3600 * fps) throw new Error('The shot cannot extend beyond one hour.');
  if (!take.panels.some((panel) => panel.id === panelId)) throw new Error('The panel is no longer in this take.');
  const frameOf = (time) => Math.ceil(time * fps - 1e-8);
  if (take.panels.some((panel) => panel.id !== panelId && frameOf(panel.time) === frame)) {
    throw new Error('Another panel already starts on that frame. Choose a different time.');
  }
  const ops = [{ update_panel: { shot: shot.id, take: take.id, panel: panelId, patch: { time: frame / fps } } }];
  const ordered = take.panels.map((panel, index) => ({ id: panel.id,
    frame: panel.id === panelId ? frame : frameOf(panel.time), index }))
    .sort((a, b) => a.frame - b.frame || a.index - b.index);
  const current = take.panels.map((panel) => panel.id);
  for (let index = 0; index < ordered.length; index++) {
    if (current[index] === ordered[index].id) continue;
    const old = current.indexOf(ordered[index].id);
    current.splice(old, 1);
    current.splice(index, 0, ordered[index].id);
    ops.push({ move_panel: { shot: shot.id, take: take.id, panel: ordered[index].id, index } });
  }
  const oldContent = contentDuration(shot, fps);
  const nextShot = { ...shot, takes: shot.takes.map((item) => item.id === take.id
    ? { ...item, panels: item.panels.map((panel) => panel.id === panelId ? { ...panel, time: frame / fps } : panel) }
    : item) };
  const nextContent = contentDuration(nextShot, fps);
  const followsContent = Math.abs(shot.duration - oldContent) < 0.5 / fps;
  const nextDuration = followsContent ? nextContent : Math.max(shot.duration, nextContent);
  if (Math.abs(nextDuration - shot.duration) > 1e-8) {
    const durationOp = { update_shot: { shot: shot.id, patch: { duration: nextDuration } } };
    // Grow before a later panel time; shrink after an earlier panel time. Reverse-order
    // Undo then restores a valid intermediate document in either direction.
    if (nextDuration > shot.duration) ops.unshift(durationOp);
    else ops.push(durationOp);
  }
  return ops;
}
