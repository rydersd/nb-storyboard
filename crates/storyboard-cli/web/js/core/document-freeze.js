// A document replacement is immutable for every panel and extension. Local previews make a
// mutable clone first, then hand it back here, so reads stay ordinary property access.
export function freezeDocument(document) {
  const pending = [document];
  const seen = new WeakSet();
  while (pending.length) {
    const value = pending.pop();
    if (!value || typeof value !== 'object' || seen.has(value)) continue;
    seen.add(value);
    Object.freeze(value);
    for (const child of Object.values(value)) if (child && typeof child === 'object') pending.push(child);
  }
  return document;
}
