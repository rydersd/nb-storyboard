export function reorderIndex(ids, draggedId, targetId, edge) {
  const from = ids.indexOf(draggedId), target = ids.indexOf(targetId);
  if (from < 0 || target < 0) return -1;
  let insertion = target + (edge === 'after' ? 1 : 0);
  if (from < insertion) insertion -= 1;
  return insertion;
}
