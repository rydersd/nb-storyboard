# Review findings log

Brief: review findings that did not become a fix round or a story, with the reason. Tags: review, workbench
Created: 2026-11-11 · Updated: 2026-11-11

## Motivation

The review cost policy caps a branch at two review rounds. Findings past the cap, findings outside the diff, and findings waiting on an owner ruling are kept here so the weekly review can pick them up.

## Open

### 2026-11-11 · workbench S10-S28 review, round 2

- **Redo can still retarget when the stale version equals the current one** (High, confirmed by running). `crates/storyboard-store/src/project.rs` `redo`. A project version is a hash of `project.json`, so a same-author edit followed by its undo can return the document to a stale tab's exact version while replacing that author's redo stack. The stale tab's redo then applies the temporary edit instead of the commit it undid. Repro: commit A, commit B, undo B (tab 1 keeps this version); tab 2 retitles A and undoes the retitle; tab 1 redoes and gets the retitle. Fix needs a unique history cursor sent with undo and redo, which changes the API contract in [workbench-api.md](../product-spec/plans/workbench-api.md). Undo has the same weakness and predates this branch (its `base_version` is unused on `main`). Proposed as one story covering both.
- **The extension boundary test is a spelling check** (High, by reading). `crates/storyboard-cli/web/tests/extension-isolation.mjs`. A dynamic `import('../js/core/' + 'api.js')` or a nested helper module passes the scan. S20 criterion 2 ("cannot write the document any other way") is not met at runtime by built-in extensions. Waiting on the owner ruling: amend S20 to a trusted-source convention with a recursive lint, or build a sandboxed frame.
- **A late async extension failure renders into a detached slot** (Medium, by reading). `crates/storyboard-cli/web/js/core/registry.js` `guard` with `crates/storyboard-cli/web/js/panels/inspector/index.js` `resetInspector`. The console still logs it; the visible error box is lost after the inspector resets. Fix: render only into a connected slot, otherwise route to the status notification; add an async-after-reset test.

### 2026-11-11 · workbench S10-S28 review, round 1

- **Taste: the player row mixes playback with structural edits.** `crates/storyboard-cli/web/js/panels/player.js`. Playback, stepping, split, merge, group, ungroup, the destructive "Split into shots", VO, image mode, export and close share one row. For the owner's review sheet.
- **Undo ignores `base_version`** (pre-existing on `main`, outside the diff). See the first round-2 entry.

## See Also

- [E2 workbench epic](../product-spec/stories/E2-workbench.md)
- [S17 redo](../product-spec/stories/S17-redo.md)
- [S20 extension points](../product-spec/stories/S20-extension-points.md)
