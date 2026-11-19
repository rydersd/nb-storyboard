# Workbench extensions

Extensions are compiled into the binary from `crates/storyboard-cli/web/extensions/`. The page imports
each built-in extension from `js/main.js`; it does not execute workspace files or download code.

An extension imports the registry and registers any of these definitions:

- `registerPanel({ id, title, region, order?, mount(el, ctx) })`
- `registerNodeBadge({ id, title?, order?, slot?, render(shot, ctx) })`
- `registerCommand({ id, title, keys?, keymap?, run(ctx, event) })`
- `registerInspectorSection({ id, title, order, pinned?, mount(el, ctx) })`

The context intentionally exposes only `store` (current document/version subscription), `selection`,
`commit(ops)`, `undo()`, registered-command dispatch, and UI event emission. It has no project path,
filesystem handle, raw fetch helper, or direct document setter. Document changes must be operation arrays
submitted with `commit`; cache and source routes remain core-owned APIs.

Registration rejects duplicate IDs. Panel/section mounting and updates are guarded independently: an
exception renders an error in that extension's slot while other panels continue. The shell records an
over-60 ms extension update with the extension ID in the console; extensions should defer non-interaction
work and keep input handlers below 60 ms.

`dialogue-word-count.js` is the sample extension. It registers a node badge and a contextual scene total;
its Node test imports the module through this compiled-in registry path.
