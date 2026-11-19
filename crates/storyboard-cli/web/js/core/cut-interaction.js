// S10's pointer path needs only an insertion target; committing stays asynchronous.
export function cutDropTarget(items, draggedIndex, hoveredIndex) {
  if (!Number.isInteger(draggedIndex) || !Number.isInteger(hoveredIndex)) return null;
  if (draggedIndex < 0 || hoveredIndex < 0 || draggedIndex >= items.length || hoveredIndex >= items.length) return null;
  return hoveredIndex;
}
