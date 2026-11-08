# nb-storyboard specification

Status: approved direction, 2026-11-07. This is the only authority for the code in this repository.

## Words

| Word | Meaning |
|---|---|
| Project | One story, one folder. |
| Scene | A named, ordered group of shots. Only a grouping: title and number are labels, never identity. |
| Shot | The working unit, and one node on the graph. Has story text, a duration and alternative takes. |
| Take | One version of a shot. Holds an ordered strip of panels. |
| Panel | One storyboard image: layers, strokes and a note. |
| Layer | One element on a panel: image, text, solid or box. |
| Cut | A named alternative ordering of shots. |
| Clip | One use of a shot (or a nested cut) inside a cut, with its own take choice and trim. |
| Sticky | A note, on the graph or pinned to something. |
| Source | Any file a user gathers into the project. |
| Frame | A unit of time only, never a picture. |

## Project folder

```
my-project/
  project.json   the document
  sources/       everything the user gathers; free-form; the tool never rewrites it
  history/       commit log and interval snapshots; local, not for git
  .cache/        disposable: index, thumbnails, renders, source scan, viewport
  .lock          advisory write lock
  .gitignore     written by `new`: history/, .cache/, .lock
```

Nothing stores an absolute path. Moving or renaming the folder changes nothing.

## Document

`project.json` is pretty-printed JSON, keys in a fixed order with `id` first, and optional fields omitted when empty. Short numeric arrays such as stroke points sit on one line. Every object keeps fields this version does not know, and writes them back in place.

- **Project:** `schema` = `"nb-storyboard/v1"`, `title`, `fps` (24, 25, 30, 60), `format {width, height}`, `scenes[]`, `cuts[]`, `active_cut?`, `stickies[]`, `layout`.
- **Scene:** `id`, `title`, `shots[]`.
- **Shot:** `id`, `title`, `action`, `dialogue`, `speaker`, `camera`, `notes`, `duration` (seconds), `status` (`draft`, `review`, `approved`, `changes`), `selected_take`, `takes[]`, `refs[]`.
- **Take:** `id`, `name`, `panels[]`, `refs[]`.
- **Panel:** `id`, `time` (seconds from shot start, below the duration), `title`, `direction`, `background`, `layers[]`, `strokes[]`.
- **Layer:** `id`, `type`, `x`, `y`, `width`, `height`, `opacity`, `hidden`, `label`, then by type: `image {src, fit: contain|cover|fill, crop?}`, `text {text, font_size, weight: regular|bold, align: left|center|right, color}`, `solid {fill}`, `box {}`.
- **Stroke:** `color`, `width`, `points[[x, y]]`. Stored and drawn over layers.
- **Cut:** `id`, `name`, `clips[]`.
- **Clip:** `id`, `source`: `shot {shot, take?}` or `cut {cut}`, `in` (frames, default 0), `out?` (frames), `slot?` (fixed length in frames).
- **Sticky:** `id`, `text`, `kind`: `note`, `todo`, `question`, `author`: `human` or `agent {name}`, `pin`, `reply_to?`, `resolved`, `created`.
- **Pin:** `none`, `scene`, `shot`, `take`, `panel`, `layer`, `clip`. Text forms: `scene:S`, `shot:H`, `take:H/T`, `panel:H/T/P`, `layer:H/T/P/L`, `clip:C/K`, `none`.
- **Layout:** `positions`: x, y for shots, stickies and nested cuts, keyed by id. No viewport: that lives in `.cache/`.

Times are RFC 3339 UTC strings everywhere.

## Order

Story order is scenes in order, then shots in order, each using its `selected_take`. It is what plays when `active_cut` is absent, and it needs no cut. A cut is an alternative path: a branch of the story. Every shot is one node however many clips use it. Clips show as numbered beads on that node.

## Timing

A shot lasts `round(duration * fps)` frames. A clip plays from `in` to `out` (default: the shot's end). With `slot`, a clip occupies exactly that many frames: shorter content is followed by black, longer content is cut. A cut used as a clip expands in place. Compiling walks a sequence and gives every item its start, end, shot, take, and offset into the shot, plus warnings. `sample(frame)` gives what is on screen at a frame. A cut that contains itself is an error. Compiling is pure and deterministic.

## Validation

One `validate(&Project)` is used by every writer. It reports all issues at once, each naming the field and the valid options.

- Ids: 1 to 160 ASCII letters, digits, `_`, `.` or `-`; not `__proto__`, `constructor` or `prototype`; unique within their kind.
- Paths: relative, 1 to 1000 characters, no `:`, `\`, control characters or `..` segment.
- Colors must parse as CSS colors.
- Limits: 500 scenes, 2000 shots, 100 takes per shot, 100 cuts, 1000 clips per cut, nesting depth 20, 10000 compiled items, 10 hours per sequence, 3600 s per shot, 20000 characters per text, format 1 to 8192 per side, positions within 100000, font size to 4096, geometry within 1e6, 200 bytes per author name.

## Operations

Ops are the only way to change a project. Each is small, named, serializable, and returns its exact inverse. Internal inverse ops are hidden from agents' published schema. Patches reject unknown fields, and name the valid ones. Required: add, remove, move and update for scenes, shots, takes, panels, layers and stickies; select take; duplicate take; add, remove, reorder and update clips; add and remove cuts; set active cut; move shot between scenes; add and remove refs; resolve and reply to stickies; move node.

A new shot gets a take and a blank panel. New objects get short readable ids (`shot-3`) and sensible defaults, so that an agent or a person never has to write a full object. Removing something a sticky is pinned to unpins the sticky and reports it. Removing a shot used in a cut is refused and names the cuts.

## Commits, conflicts, history

`commit(dir, base_version, ops, author, note?)`. A version is the sha256 of `project.json`. Stale commits rebase when their footprints do not overlap those landed since the base, and otherwise fail with a `reason`, and compact summaries of what intervened. Footprints name exactly what an op touches; removing something touches only the stickies pinned to it. Validation runs before the conflict check, so that the real error shows first. A hand edit found on disk is logged as an external commit, and does not break later rebases.

`history/log.jsonl` holds ops, inverses, author, time, note and a generated one-line summary. Snapshots are written every 50 commits, not per commit. A torn log tail is repaired on open. Undo commits the inverses, under the same rebase rule.

Agents cannot land a sticky under any author but themselves, and nothing changes the author of a stored sticky. Without `--author`, a non-terminal CLI write is refused, unless `NB_AUTHOR` is set.

## Sources

Any file may be in `sources/`, in any folder. There is no manifest. A scan writes `.cache/sources.json`: path, kind, size, modified time, dimensions, and what uses it. `refs` on shots and takes attach files. Importing copies in, never overwrites unasked, and never leaves the project. Images in PNG, JPEG, WebP, GIF, TIFF and SVG can be placed. The watcher sees subfolders.

## Rendering

- `panel_image` renders a panel at a width. Modes: `stage` (the image only), and `board` (the image plus a caption strip with shot number, action, dialogue and timing).
- `cut_sheet` renders a contact sheet of a sequence.
- A panel with no content renders a visible empty placeholder, never plain black.
- A missing or undecodable source draws a labelled box, the rest of the panel renders, and the result carries `warnings`. So do text overflow and unsupported values.
- `weight` is honoured. Strokes are drawn. Line widths and labels scale with the format. Downscaling keeps thin lines.
- Canvas limit 64 megapixels; output width 1 to 8192. The bundled font is used, and output matches across machines.
- Render caches are keyed by the panel's content and its source files' modified times, not by the project version, and are pruned.

## Workspace

A workspace is a folder whose children are projects. The index holds per project only: path, title, scene and shot counts, duration, unresolved stickies, last author, aspect, modified time, and a thumbnail of the first non-empty panel. It reads cache files on a warm start, never whole documents. Only the active project is held in memory.

## Command line and MCP

One name per concept across CLI, MCP and ops: `project`, `base_version`, `panel`, `clip`. The CLI reads as verbs for people: `new`, `add-scene`, `add-shot`, `add-panel`, `add-text`, `add-image`, `import`, `sources`, `move-shot`, `select-take`, `trim`, `undo`, `history`, `show` (a readable table of the sequence), `panel-image`, `cut-sheet`, `note`, `reply`, `resolve`, `notes`, `schema`. Every flag has help text. Output is JSON, and `--pretty` gives text for people. Commands may address a panel by sequence position (`--at 3`).

MCP tools mirror these. Instructions carry a short working agreement: how to read, how to write, what stickies mean. `panel_image` returns an image block as well as a path. Write results stay compact unless `full` is asked for. Tool descriptions say what each tool is for, once.

Errors are JSON with `kind`, `message`, `hint`, and `valid` where there are options, all issues at once.

## Distribution

Tagged releases publish a download that runs without developer tools. Mac first (Apple Silicon and Intel), then Windows x64.

## Deferred to the UI plan

Server, browser UI, live updates to a screen, auto-layout drawing, and fallback fonts.
