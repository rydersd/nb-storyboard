# S27: Remember the view in the project

Status: specced (owner approved 2026-11-10: "write the api gaps ... finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: API gaps found while building S9, 2026-11-08.

## Story

The graph's pan, zoom and viewed cut are kept in the project's disposable `.cache/`, as the spec says, so they follow the project between browsers.

## Acceptance

1. `GET /api/viewport?project=P` and `POST /api/viewport` read and write `.cache/viewport.json` (pan x/y, zoom, viewed cut); not a commit, not in history, not in `project.json`.
2. Writes are bounded (small JSON, validated numbers, a known cut or null) and debounced by the page (at most one per second while panning).
3. The page restores the saved view on open; localStorage stays only as a fallback when the route fails.
4. A missing or corrupt file is treated as no saved view.
