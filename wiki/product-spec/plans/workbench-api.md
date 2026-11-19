# Workbench HTTP API

Brief: the contract between `storyboard serve` and the page it embeds. Tags: workbench, api, S9
Created: 2026-11-07 · Updated: 2026-11-10

Story: [S9 the workbench interface](../stories/S9-workbench.md). Interaction design: [workbench-ux.md](workbench-ux.md).

## Overview

`storyboard serve [WORKSPACE] [--port 7420] [--no-open]` listens on 127.0.0.1 only and opens the browser. The page (`crates/storyboard-cli/web/index.html`, `app.js`, `app.css`) is compiled into the binary with `include_str!`; there is no build step and nothing is loaded from the network. Requests whose `Host` is not `127.0.0.1:PORT` or `localhost:PORT` get 403 (DNS-rebinding guard); POSTs with a foreign `Origin` get 403.

Every document write goes through `ops_api::commit_ops`, the same path as the CLI and MCP, with author `human`. The page never edits the document itself: it sends ops and replaces its copy with the `document` the server returns. Explicit cache and source-import routes are the exceptions: they write only their documented non-document files.

## Routes

| Route | Result |
|---|---|
| `GET /api/workspace` | `{root, root_path, cli_path, projects: [{project, title, fps, scene_count, shot_count, duration, unresolved_stickies, last_author, aspect, modified, thumbnail, problem?}]}`. `project` is the folder name and the key for every other route. `duration` is in frames and `fps` converts it to seconds. `thumbnail` is a URL or null. The CLI `index [WORKSPACE]` and MCP `index` tool return the same cached project entries, including `fps`. |
| `POST /api/new-project` | Body `{project, title, fps?, width?, height?}`. Creates only a new direct child of the served workspace; `project` is 1–64 ASCII letters, numbers, hyphens or underscores. Defaults match `storyboard new` (24 fps, 1440×900). A collision or invalid value is refused. Returns `{project, version, title}`. |
| `POST /api/upload-movie?project=P&folder=UUID` | Copies a bounded MP4 body into the project and returns its media probe and source path. |
| `POST /api/import-movie` | Body `{project, base_version, folder, scene?, title, interval, cut_times?}`. `cut_times` is an optional ascending array of seconds; when present and nonempty, it overrides interval sampling, rounds marks to project frames, and creates one source-linked shot per span in one undoable commit. |
| `GET /api/project?project=P[&cut=C]` | `{project, version, document, sequence: {cut, frames, items: [{position, start, end, shot, take, title, duration, offset, clip}], warnings}}`. `document` is `project.json` as the spec defines it; optional fields are omitted when empty, so default them (`cuts`, `stickies`, `active_cut`, shot `action`/`dialogue`/`speaker`/`camera`/`notes`/`status`/`refs`, panel `layers`/`strokes`/`title`, `layout.positions`). Without `cut` the sequence is the active one. |
| `POST /api/commit` | Body `{project, base_version, ops: [Op], note?}`. 200: `{version, summary, rebased, ids, document}`. |
| `POST /api/undo` | Body `{project, base_version, steps?}`. Undoes the person's own latest commits. 200: `{version, summary, rebased, document}`. |
| `POST /api/redo` | Body `{project, base_version, steps?}`. Re-applies the person's most recently undone commits; `steps` defaults to 1. Redo is per author, uses the undo rebase/conflict rules, and a new commit by that author clears their redo. 200: `{version, summary, rebased, document}`. |
| `GET /api/viewport?project=P` | `{viewport: null}` when no valid saved view exists, otherwise `{viewport: {pan_x, pan_y, zoom, viewed_cut}}`. `viewed_cut` is a cut id or null. Missing, corrupt, oversized or no-longer-valid cache data is treated as no saved view. |
| `POST /api/viewport` | Body `{project, viewport: {pan_x, pan_y, zoom, viewed_cut}}`. Writes only `.cache/viewport.json`, atomically; it never commits or changes `project.json` or history. The body has no unknown fields, serialized viewport data is at most 4096 bytes, pan values are finite and within ±1,000,000, zoom is 0.1–4, and `viewed_cut` must name an existing cut or be null. Returns `{viewport}`. |
| `GET /api/sources?project=P` | `{sources: [{path, kind, size, modified, dimensions, used_by: [{target}]}], missing_refs: [{path, target}]}` from the store source scan. |
| `POST /api/upload-source?project=P&folder=ID&name=FILE` | Imports the bounded image body beneath `sources/imported-shots/ID/FILE` through the store import path. It creates a new file and refuses a collision; it never overwrites an existing source. Returns `{path}`. |
| `GET /api/panel.png?project=P&shot=S[&take=T][&panel=N]&width=W[&mode=stage\|board]` | PNG. Take defaults to the selected take, panel to its first. `X-Render-Warnings` header holds a JSON array. Cached in memory by content, so a repeat costs a few ms; a fresh render costs tens of ms. Width 16–4096. |
| `GET /api/thumb.png?project=P` | The workspace index thumbnail. |
| `GET /api/history?project=P[&limit=N]` | The commit log, newest first. |
| `GET /api/schema[?op=NAME]` | JSON Schema of every public op. |
| `GET /api/events` | Server-sent events. `project`: `{project, version, author, summary}` after any write (CLI, MCP, the page itself or a hand edit; author is `human`, `agent:NAME` or null for a hand edit). `source`: `{project, path}` when a source file changes. `workspace`: `{}` when projects are added or removed. |

## Errors

Every failure is `{error: {kind, message, hint, valid, issues?}}`. Status: 400 `validation`/`usage`/`authorship`/`empty_commit`, 404 `not_found` (e.g. nothing of yours to undo), 409 `conflict` (`issues[0]` is `{current_version, intervening: [{version, author, summary, ...}]}`), 422 `render`/`compile`, 500 otherwise. `valid` lists the accepted values; show them.

## Ops

Ops are externally tagged JSON, one key per op: `{"move_node": {"node": "shot:shot-1", "position": {"x": 120, "y": 40}}}`. `GET /api/schema` is the full list. The ones the page uses:

- `move_node {node, position|null}`: node is `shot:ID`, `sticky:ID` or `cut:ID`; null clears the stored position. Stored under `document.layout.positions["shot:ID"]`.
- `move_shot {shot, index}` (within its scene), `move_shot_between_scenes {shot, scene, index}`.
- `add_scene {title, index?}`, `update_scene {scene, patch}`, `add_shot {scene, title, index?}` (gets a take and blank panel), `remove_shot {shot}`.
- `update_shot {shot, patch}`: patch keys `title, action, dialogue, speaker, camera, notes, duration, status` (`draft|review|approved|changes`).
- `add_take {shot, name}`, `duplicate_take`, `select_take {shot, take}`, `add_panel {shot, take, index?}`, `update_panel {shot, take, panel, patch}`.
- `add_sticky {text, kind: note|todo|question, author: "human", pin, created}`: `pin` is text (`none`, `shot:H`, `take:H/T`, `panel:H/T/P`, `scene:S`); `created` is an RFC 3339 UTC time. `reply_sticky {sticky, text, kind, author: "human", created}`, `resolve_sticky {sticky}`.
- `set_active_cut {cut|null}`.
- `batch {ops: [...]}`: one commit, one undo step.

A sticky under any author but `human` is refused (`authorship`).

## Conflicts

Send the `version` you last saw as `base_version`. A stale write whose footprint does not overlap what landed since is rebased and succeeds (`rebased: true`). An overlapping one fails with 409; show the intervening summaries and the current value, and let the person re-apply.

## See Also

- [S9 the workbench interface](../stories/S9-workbench.md)
- [workbench-ux.md](workbench-ux.md)
- [The specification](../spec.md)
