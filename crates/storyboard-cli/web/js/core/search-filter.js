const text = (value) => String(value || '').toLocaleLowerCase();
const shotStatus = (shot) => ['draft', 'review', 'approved', 'changes'].includes(shot.status) ? shot.status : 'draft';

export function filterShotIds(model, { query = '', status = '', unresolved = false } = {}) {
  const needle = text(query).trim();
  const matches = new Set();
  for (const id of model.order) {
    const shot = model.shots.get(id)?.shot;
    if (!shot) continue;
    if (status && shotStatus(shot) !== status) continue;
    if (unresolved && !(model.pinnedToShot.get(id) || []).some((sticky) => !sticky.resolved)) continue;
    const haystack = [shot.title, shot.action, shot.dialogue, shot.notes].map(text).join('\n');
    if (!needle || haystack.includes(needle)) matches.add(id);
  }
  return matches;
}
