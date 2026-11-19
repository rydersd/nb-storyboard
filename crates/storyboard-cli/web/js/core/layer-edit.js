export function layerHit(layers, x, y) {
  for (let index = layers.length - 1; index >= 0; index -= 1) {
    const layer = layers[index];
    if (layer.hidden) continue;
    if (x >= layer.x && y >= layer.y && x <= layer.x + layer.width && y <= layer.y + layer.height) return layer;
  }
  return null;
}

export function layerDragPatch(layer, handle, dx, dy) {
  const minimum = 1;
  if (handle === 'move') return { x: Math.round(layer.x + dx), y: Math.round(layer.y + dy) };
  if (handle === 'nw') {
    const width = Math.max(minimum, layer.width - dx), height = Math.max(minimum, layer.height - dy);
    return { x: Math.round(layer.x + layer.width - width), y: Math.round(layer.y + layer.height - height), width: Math.round(width), height: Math.round(height) };
  }
  if (handle === 'ne') {
    const width = Math.max(minimum, layer.width + dx), height = Math.max(minimum, layer.height - dy);
    return { y: Math.round(layer.y + layer.height - height), width: Math.round(width), height: Math.round(height) };
  }
  if (handle === 'sw') return { x: Math.round(layer.x + layer.width - Math.max(minimum, layer.width - dx)), width: Math.round(Math.max(minimum, layer.width - dx)), height: Math.round(Math.max(minimum, layer.height + dy)) };
  return { width: Math.round(Math.max(minimum, layer.width + dx)), height: Math.round(Math.max(minimum, layer.height + dy)) };
}
