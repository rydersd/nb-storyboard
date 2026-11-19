const shotsIn = (doc) => Array.isArray(doc?.scenes)
  ? doc.scenes.flatMap((scene) => scene.shots || [])
  : Array.isArray(doc?.shots) ? doc.shots : [];

export function historyShotId(record, doc) {
  const current = new Set(shotsIn(doc).map((shot) => shot.id));
  for (const footprint of record?.footprints || []) {
    // Operation footprints are stable paths: shot/H/…, take/H/T, panel/H/T/P, layer/H/T/P/L.
    const parts = String(footprint).split('/');
    const shotId = ['shot', 'take', 'panel', 'layer'].includes(parts[0]) ? parts[1] : null;
    if (shotId && current.has(shotId)) return shotId;
  }
  const summary = String(record?.summary || '');
  for (const shot of shotsIn(doc)) {
    for (const candidate of [shot.id, shot.title].filter(Boolean)) {
      const escaped = String(candidate).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      if (new RegExp(`(^|[^A-Za-z0-9_.-])${escaped}($|[^A-Za-z0-9_.-])`, 'i').test(summary)) return shot.id;
    }
  }
  return null;
}
