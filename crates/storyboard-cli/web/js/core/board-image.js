// Replace the full-canvas board image while leaving annotations and overlay layers intact.
export function boardImageOp(shot, take, panel, format, src) {
  const base = panel.layers.find((layer) => layer.type === 'image'
    && layer.x === 0 && layer.y === 0
    && layer.width === format.width && layer.height === format.height);
  return base
    ? { update_layer: { shot, take, panel: panel.id, layer: base.id, patch: { src } } }
    : { add_layer: { shot, take, panel: panel.id, layer_type: 'image',
      fields: { src, fit: 'contain' }, index: 0 } };
}
