// Pure panel-dock model. DOM code owns pointer capture and persistence; this module is safe to
// exercise in Node and deliberately does no layout reads while a pointer is moving.
export const docks = ['left', 'right', 'bottom', 'center', 'hidden'];
const sizedDocks = ['left', 'right', 'bottom'];
const minSize = 160;
const maxSize = 800;

export function layoutFor(ids) {
  const unique = [...new Set(ids)];
  return { left: [], right: unique, bottom: [], center: [], hidden: [], sizes: {} };
}

export function clampDockSize(dock, size) {
  if (!sizedDocks.includes(dock) || !Number.isFinite(size)) return minSize;
  return Math.round(Math.max(minSize, Math.min(maxSize, size)));
}

export function normalizeLayout(value, ids) {
  const allowed = new Set(ids);
  const seen = new Set();
  const result = { left: [], right: [], bottom: [], center: [], hidden: [], sizes: {} };
  for (const dock of docks) {
    for (const id of Array.isArray(value?.[dock]) ? value[dock] : []) {
      if (allowed.has(id) && !seen.has(id)) { result[dock].push(id); seen.add(id); }
    }
  }
  for (const id of ids) if (!seen.has(id)) result.right.push(id);
  for (const dock of sizedDocks) {
    const size = value?.sizes?.[dock];
    if (Number.isFinite(size) && size >= minSize && size <= maxSize) result.sizes[dock] = Math.round(size);
  }
  return result;
}

export function movePanel(layout, id, destination) {
  if (!docks.includes(destination)) return layout;
  const ids = docks.flatMap((dock) => Array.isArray(layout?.[dock]) ? layout[dock] : []);
  if (!ids.includes(id)) return layout;
  const next = normalizeLayout(layout, ids);
  for (const dock of docks) next[dock] = next[dock].filter((item) => item !== id);
  next[destination].push(id);
  return next;
}

export function resizeDock(sizes, dock, delta) {
  if (!sizedDocks.includes(dock) || !Number.isFinite(delta)) return { ...sizes };
  return { ...sizes, [dock]: clampDockSize(dock, (sizes?.[dock] || defaultDockSize(dock)) + delta) };
}

export function defaultDockSize(dock) {
  return dock === 'bottom' ? 240 : 320;
}

/** The dock zone nearest a pointer. A 28px inner dead-zone avoids accidental re-docking. */
export function dropDock(rect, x, y) {
  const distances = [
    ['left', x - rect.left], ['right', rect.right - x], ['bottom', rect.bottom - y],
  ].filter(([, distance]) => distance >= 0);
  distances.sort((a, b) => a[1] - b[1]);
  const [dock, distance] = distances[0] || [];
  return distance !== undefined && distance <= 96 ? dock : null;
}
