// Browser preflight and stable shot placement for an image sequence import.

const supported = /\.(png|jpe?g|webp|gif|tiff?|svg)$/i;
export const MAX_SHOT_IMAGES = 500;
export const MAX_SHOT_IMAGE_BYTES = 32 * 1024 * 1024;

export function imageSelectionError(files) {
  if (!files.length || files.length > MAX_SHOT_IMAGES) return `Select 1 to ${MAX_SHOT_IMAGES} image files.`;
  const names = new Set();
  for (const file of files) {
    if (!supported.test(file.name) || names.has(file.name.toLowerCase())) {
      return `${file.name}: unsupported or duplicate filename.`;
    }
    if (!file.size || file.size > MAX_SHOT_IMAGE_BYTES) {
      return `${file.name}: image must be above 0 and at most 32 MB.`;
    }
    names.add(file.name.toLowerCase());
  }
  return '';
}

export function insertionAfterShot(doc, sceneId, afterShot) {
  if (!afterShot) return null;
  const scene = doc?.scenes.find((item) => item.id === sceneId);
  const index = scene?.shots.findIndex((shot) => shot.id === afterShot) ?? -1;
  if (index < 0) throw new Error('The selected shot moved or was removed during import. Select it again and retry.');
  return index + 1;
}
