# E2: The workbench

Status: movie-revision implementation active (2026-11-09); see the [delivery ledger](../../development/workbench-ledger.md) for source-bound checks and remaining decisions

The browser interface where a person art-directs and resequences a project while an agent edits the same project from its own terminal. See [the specification](../spec.md), section "Deferred to the UI plan", the [UX plan](../plans/workbench-ux.md) and the [HTTP API](../plans/workbench-api.md).

## Stories

| Story | What | Status |
|---|---|---|
| [S9](S9-workbench.md) | The workbench: graph, inspector, stickies, live updates, conflicts | implemented baseline; owner acceptance separate |
| [S10](S10-cut-editing.md) | Edit cuts and clips in the graph | cut reorder and boundary edits implemented; every-interaction latency and owner acceptance pending; latency criterion browser-measured 2026-11-10: p95 13.8-26.4 ms at 18 and 300 shots |
| [S11](S11-layer-editing.md) | Edit layers on the panel preview | browser-accepted 2026-11-10 (single run) |
| [S12](S12-source-browser.md) | Browse sources and place images | browser-accepted 2026-11-10 except the native drag-start leg; duplicate-ref and layer-scan fixes landed with tests |
| [S13](S13-reorder-panels-takes-scenes.md) | Reorder panels, takes and scenes by drag | browser-accepted 2026-11-10 (single run) |
| [S14](S14-history-view.md) | See the change history | browser-accepted 2026-11-10 (single run) |
| [S15](S15-new-project.md) | Create, close and reopen projects | implemented; focused browser and package checks passed, owner acceptance pending |
| [S16](S16-search-filter.md) | Search and filter shots | browser-accepted 2026-11-10 (single run) |
| [S17](S17-redo.md) | Redo (needs a spec amendment first) | browser-accepted 2026-11-10 (single run) |
| [S18](S18-stills-flipthrough.md) | Stills-only flip-through | superseded by S22 |
| [S19](S19-panel-layout.md) | Arrange the workbench in panels | browser-accepted after page fixes 2026-11-10; Option-key shortcuts now match key codes (tested), physical-keyboard check open |
| [S20](S20-extension-points.md) | Extension points (needs a ruling on where extensions load from) | browser-accepted 2026-11-10; read-only document view landed with tests |
| [S21](S21-numbered-image-shot-import.md) | Import numbered images as one shot | implemented; focused browser import passed, owner acceptance pending |
| [S22](S22-player-timing-intercut.md) | Play and retime an audiovisual intercut | player, timing and cut editing implemented; exact scene clocks and cross-cut audio pending owner rulings |
| [S23](S23-movie-to-board-import.md) | Import a rough MP4 as editable boards | timed import, visual cut candidates and manual marks implemented; owner acceptance pending |

| [S26](S26-index-fps.md) | Project lengths in time (fps in the index) | browser-accepted 2026-11-10 (single run) |
| [S27](S27-viewport-cache.md) | Remember the view in `.cache/` | browser-accepted 2026-11-10 (single run) |
| [S28](S28-readable-summaries.md) | Readable change summaries | implemented; tests pass; not separately browser-checked |

## Not yet stories

Small items from the UX plan, to fold into a story when one touches them: per-agent colours when several agents write at once; screen-reader access to the graph beyond the keyboard map; browser fallback fonts.

## Non-goals

A chat box, drawing or painting tools, Windows packaging (S7 phase 2). Audiovisual playback and timing are tracked in S22, beyond the original S9 workbench scope.
