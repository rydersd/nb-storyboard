export function sourceRefOps(target, reference, add) {
  const refTarget = target.kind === 'take' ? { take: { shot: target.shot, take: target.take } } : { shot: { shot: target.shot } };
  return [{ [add ? 'add_ref' : 'remove_ref']: { target: refTarget, reference } }];
}

/** Place a new source asset without silently replacing an existing board image. */
export function sourceImageLayerOp(shot, take, panel, src) {
  return { add_layer: { shot, take, panel, layer_type: 'image', fields: { src, fit: 'contain' } } };
}

export function sourceLabel(source) {
  const bytes = Number(source.size || 0);
  const size = bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(bytes < 10240 ? 1 : 0)} KB`;
  return `${source.kind || 'file'} · ${size}`;
}

/**
 * The ref through which the selected shot or take uses a source, as an attach/detach target, or null.
 * Matches whole targets (`take:SHOT/TAKE`, then `shot:SHOT`), so shot-1 never matches shot-10.
 */
export function sourceUseFor(source, target) {
  if (!target) return null;
  const targets = new Set((source?.used_by || []).map((use) => String(use.target || '')));
  if (target.kind === 'take' && targets.has(`take:${target.shot}/${target.take}`)) return { kind: 'take', shot: target.shot, take: target.take };
  if (targets.has(`shot:${target.shot}`)) return { kind: 'shot', shot: target.shot };
  return null;
}
