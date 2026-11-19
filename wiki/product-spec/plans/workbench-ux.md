# Workbench interaction design

Brief: buildable UX for the browser workbench (vanilla JS, HTML/CSS/SVG, no build step). Spec: [S9](../stories/S9-workbench.md), [specification](../spec.md).
Created: 2026-11-07. Status: proposal for owner review. Op names are the snake_case `Op` variants in `crates/storyboard-core/src/ops.rs`.

Two principles carry the whole design:
- **Looking never writes.** Viewing a cut, previewing a take, panning, selecting: no commit. Only an explicit act commits.
- **Position is where; the line is the story.** Dragging a node only moves it. Dropping it *on the story line* is the one way to reorder by mouse, and a label says which will happen before you let go.

## 1. Layout

```
+--------------------------------------------------------------------------------+
| [My Film v]  [Path: Story order v]                 [+ Shot] [+ Note]   * Live  |  toolbar 40px
+--------------------------------------------------------+-----------------------+
| GRAPH (flex, min 50%)                                  | INSPECTOR (30%, 360   |
|  +- 1  Opening ------------------------------+         |  min, drag to resize) |
|  | [##]-->[##]-->[##]-->[##]    (+)          |         | 2.3  Title    [Review]|
|  +-------------------------------------------+         | +-------------------+ |
|      `-------------------.                             | |  panel preview    | |
|  +- 2  Chase ------------v---+    [sticky]             | |  (sticky on top)  | |
|  | [##]-->[##]-->[##]   (+)  |                         | +-------------------+ |
|  +---------------------------+                         | [p1][p2][p3] (+)      |
|  (+ Scene)                                             | Takes  A*  B   (+)    |
|                                                        | fields / Layers /     |
|                                                        | Stickies thread       |
+--------------------------------------------------------+-----------------------+
| codex: moved shot-4 to Chase, 3s ago               Drop on line to reorder  64% |  status 24px
+--------------------------------------------------------------------------------+
```

- **Toolbar:** project switcher (section 6), path picker (section 2.4), `+ Shot`, `+ Note`, connection pill (`Live` / `Reconnecting` / `Offline`). Nothing else.
- **Graph:** the canvas. Scenes, shots, story line, cut path, free stickies.
- **Inspector:** always present; shows the selected shot (section 4). `Cmd/Ctrl+\` collapses it. With no selection it shows the project summary (title, counts, duration, unresolved stickies).
- **Status bar:** left is the latest change by anyone (click to frame it), or the current error/hint; right is a contextual hint, zoom level and `?` for keys.
- No sidebars, no tabs, no modals except the project switcher popover and the keyboard map.

## 2. Graph

Rendering: one transformed `<div>` world (CSS `transform: translate() scale()`) holding HTML node cards, with an SVG underlay in the same coordinates for frames, lines and beads. Pan and zoom only change one transform.

### 2.1 Shot node (176 px wide at 100%)
- Thumbnail: first panel of the selected take, `stage` render, at project aspect (176 x 99 for 16:9). Grey `--surface-2` box with a thin progress bar while it loads.
- 3 px left edge in the status colour (draft, review, approved, changes).
- Line 1: sequence number `2.3` (mono, faint) then title, one line, ellipsis.
- Line 2 (mono, 11 px): duration `3.5s`, take count `3 tk` (hidden when 1), sticky count with unresolved dot (hidden when 0).
- Ports: a 6 px dot mid-left and mid-right where the line enters and leaves.
- Clip beads: only while a cut is viewed. 16 px teal circles with the clip's 1-based number in that cut, along the node's top edge; a shot used twice shows two beads.
- Level of detail: under 45% zoom hide text lines; under 20% draw a status-coloured rectangle only.
- States: hover lifts the border to `--border-strong`; selected gets a 2 px `--select` ring; remote-changed flashes (section 5).

### 2.2 Scene frame
- Rounded rectangle drawn around its shots' bounding box, 16 px padding, 28 px header with number and title. Fill `--frame`, no strong border; frames recede.
- The frame is derived from its shots, never stored. During a drag frames are frozen at drag-start geometry and recompute on drop.
- A `(+)` ghost slot appears after the last shot on frame hover: adds a shot at the end of that scene.
- Empty scene: frame at its auto-layout row with only the ghost slot and "Empty scene".

### 2.3 Story line
- Story order (scenes in order, shots in order) drawn as wires from each node's right port to the next node's left port: cubic Bezier, 2 px `--line`, small arrowhead at entry. Scene-to-scene wires curve from the end of one row to the start of the next.
- It is always drawn. It is the thing you drop on to reorder (section 3.1).

### 2.4 Cuts: viewing and choosing
- The toolbar **Path picker** lists `Story order` and every cut by name. The active cut has a filled dot.
- Picking an entry only *views* it (UI state, kept in the viewport cache): the cut path draws in `--cut` teal through nodes in clip order, beads appear, the story line drops to 25% opacity, shots not in the cut drop to 45%.
- The last menu item, `Make "<name>" the active cut` (or `Play story order` to clear), sends `set_active_cut`. This is the only control that changes what plays.
- A nested cut used as a clip draws as a 140 x 36 teal-outlined pill with the cut's name at its stored layout position (or after its preceding clip); double-click views that cut.
- While a cut is viewed, dragging a numbered clip bead onto another bead reorders that placement. Dragging the shot card itself only moves its graph position; dropping on the cut path does not edit clips.

### 2.5 Stickies on canvas
- Free stickies (`pin: none`) are 200 px cards at their layout position: `--sticky` fill, 3 px top bar in the author colour, kind glyph (note, todo `[ ]`, question `?`), first 4 lines, author label (`You` or the agent name), reply count.
- Pinned stickies do not float. A shot node shows their count; the thread is in the inspector. Scene-pinned stickies show as a count chip in the frame header. Resolved stickies are hidden on canvas.

### 2.6 Auto-layout (shots with no stored position)
- Scenes are rows, top to bottom in story order; shots run left to right in story order, like a board page. Row y = sum of previous rows' heights + 64 px gap; shot x = slot index x (176 + 40).
- A shot with a stored position sits there; its unpositioned siblings keep their slot. A free sticky with no position stacks in a column 80 px right of the widest row.
- "Tidy" (context menu, `Shift+T`) clears the selected nodes' positions (`move_node` with no position, in one `batch`) so they return to the auto slots.

## 3. Interactions

### 3.1 Reorder versus reposition (the key question)
While a shot node is dragged the app checks the pointer every frame:
1. **On the line:** pointer within 16 screen px of a story-line wire, or anywhere inside a *different* scene's frame (snaps to that scene's nearest gap). The wire splits open with a 24 px gap marker in `--select`, and a chip by the cursor reads `Insert after 2.3` (or `Start of Chase` / `End of Opening` on a scene-to-scene wire, by which half you are over).
2. **Anywhere else:** chip reads `Move`. Nothing else changes.
3. **Alt/Option held:** always `Move` (matches Blender's "don't auto-insert").

On drop, `Move` sends `move_node {node, position}`. `Insert` sends one `batch`: `move_shot {shot, index}` (same scene) or `move_shot_between_scenes {shot, scene, index}`, plus `move_node` to the drop point only if the shot already had a stored position (auto-laid-out shots stay auto and reflow into their new slot). One commit, one undo step. Numbers on every node renumber at once.
Discoverability: the status bar hint during every drag says "Drop on the line to reorder, Alt to only move", the chip names the result before release, and `Alt+Left/Right` reorders from the keyboard.

### 3.2 Gesture table

| Gesture | Result | Op |
|---|---|---|
| Click node / Shift-click / marquee on empty canvas | Select / add to selection / box-select; inspector follows the primary | none |
| Drag node(s), `Move` | Nodes move; multi-select moves together | `move_node` each, in one `batch` |
| Drag node, `Insert` | Reorder in story (section 3.1) | `move_shot` or `move_shot_between_scenes` (+ `move_node`), `batch` |
| `Alt+Left/Right` on a shot | Earlier/later by one; crosses into the neighbouring scene at its edge | `move_shot` / `move_shot_between_scenes` |
| Drag scene frame header | Moves all its shots | `move_node` per shot, `batch` |
| Double-click frame header | Rename inline; Enter commits, Esc cancels | `update_scene` |
| `+ Shot`, `A`, ghost `(+)` | New shot after selection (or end of last scene); selected, title focused | `add_shot` |
| `Shift+A`, context menu, `(+ Scene)` | New scene after the selected one (or last), title in edit | `add_scene` |
| `+ Note`, `N` with nothing selected | Free sticky at the pointer (or view centre), text in edit | `add_sticky` (+ `move_node`), `batch` |
| `N` with a shot selected | Sticky pinned to that shot, opens in the inspector thread | `add_sticky` pin `shot:H` |
| Drag free sticky | Reposition | `move_node` |
| Drop free sticky on a node | Pin to that shot | `update_sticky` patch `pin` |
| Drop free sticky on a panel thumbnail / take tab (inspector) | Pin to that panel / take | `update_sticky` patch `pin` |
| Thread item menu, `Unpin` | Becomes free, placed 40 px right of its node | `update_sticky` + `move_node`, `batch` |
| Reply field, Enter | Reply in thread | `reply_sticky` |
| `Resolve` on a thread root | Thread collapses, hidden on canvas | `resolve_sticky` |
| `Delete`/`Backspace` on selection | Removes shots, stickies, or a selected scene; status bar names what went, with "Cmd+Z to undo" | `remove_shot` / `remove_sticky` / `remove_scene`, `batch` |
| Path picker entry | View a cut | none |
| Path picker `Make active` | Sets what plays | `set_active_cut` |
| `Shift+T` / context `Tidy` | Back to auto-layout | `move_node` no position, `batch` |
| `Cmd/Ctrl+Z` | Undoes **your** last commit, never the agent's; status bar names it | store `undo` (author = human) |
| `Esc` during a drag | Cancels; node returns; nothing sent | none |
| Double-click node | Frame it and select | none |

No confirmation dialogs: everything is one undo away. A refusal from the server (for example removing a shot used in a cut) leaves the graph unchanged and shows the server's `message` and `hint` in the status bar, for example "Can't remove shot-4: used in cuts Teaser, Alt ending".

### 3.3 Canvas navigation
Space+drag, middle-drag, or two-finger trackpad scroll pans. Mouse wheel zooms at the pointer; pinch (wheel with `ctrlKey`) zooms; `Cmd/Ctrl+wheel` always zooms. Heuristic: a wheel event with `deltaX != 0` or a fractional `deltaY` is a trackpad pan; integer `deltaY` only is a mouse wheel zoom. Zoom 10% to 400%. Left-drag on empty canvas is the marquee. Viewport (pan, zoom, viewed cut, inspector width) is saved per project in `.cache/`.

## 4. Shot inspector

Top to bottom; the header and preview stay pinned while the rest scrolls.
1. **Header:** `2.3` (mono), title (inline text field), status pill (menu of the four statuses, keys `1`-`4`). `update_shot`.
2. **Preview:** the selected panel at inspector width x devicePixelRatio, `board` mode off (`stage`). The previous render stays visible at 60% with a 2 px progress bar while the new one loads; never a blank flash. Double-click (or `Enter` on the preview) fills the graph area with the panel for review; `Left/Right` steps panels, `Esc` returns.
3. **Panel strip:** thumbnails of the viewed take's panels with their `time`; click selects, `[`/`]` step. Trailing `(+)` adds a panel after the selected one (`add_panel`). `Delete` on a focused thumbnail removes it (`remove_panel`). Under the strip, the selected panel's `time` (s) and `direction` (one line) edit in place (`update_panel`).
4. **Takes:** tabs `A* B C (+)`. The selected take has a dot. Clicking another tab *previews* it (preview, strip and layers switch; nothing is written) and shows one button: `Use take B` (`select_take`). `(+)` adds a blank take (`add_take`); tab menu has `Duplicate` (`duplicate_take`) and `Remove` (`remove_take`).
5. **Fields:** action and dialogue (multi-line, auto-grow), speaker, camera (single line), duration (seconds; frames shown beside it, `84f @ 24`; Up/Down nudge 0.1 s, Shift 1 s), shot notes (multi-line, labelled "Shot notes" to keep it apart from stickies). Commit on blur, Enter (single line) or Cmd/Ctrl+Enter (multi-line); Esc reverts the field. Validation errors show under the field with the server's `hint` and `valid` options.
6. **Layers** (collapsed by default, count in the heading): one row per layer, top first: type glyph, label, `hidden` shown as a struck eye. Hovering a row outlines that layer's box on the preview. Read-only in S9.
7. **Stickies:** threads pinned to this shot, its takes, and its panels, grouped with a small `Take B` / `Panel 3` label, newest activity first. Each item: author label in author colour, kind glyph, text, relative time; root items get `Resolve` and a reply field. `Show resolved` toggle at the bottom. New sticky field at the top: kind selector (note, todo, question) and text; pins to the shot, or to the selected panel when the strip has focus.

## 5. Live agent changes and conflicts

- The server pushes an event per commit (version, author, summary, touched ids). The client refetches and patches only the touched nodes and the inspector.
- **Highlight:** each changed node gets a 1.2 s ring in `--agent` that fades out, and an author chip (`codex`) above it for 3 s. Changed off-screen nodes are not chased; the status bar shows `codex: moved shot-4 to Chase, 3s ago` (the log's one-line summary), and clicking it frames those nodes.
- **No focus theft:** selection, viewport, scroll, and caret never move because of a remote change. If the selected shot is removed remotely the inspector shows "Removed by codex" and the selection clears.
- **Field being edited:** a remote change to the focused field never replaces your text. A small `--agent` dot appears with "codex changed this"; other fields update in place with a 1.2 s tint.
- **Conflict (AC7):** every commit carries the client's `base_version`. If the server rebases, nothing is shown. If it refuses, the field (or node) shows a conflict bar in `--conflict`: "codex changed this first", the current value, and two buttons: `Keep theirs` (discard your draft) and `Use mine` (re-commit your value on the new version). The current state is always what is displayed; your value is only kept as the draft. A refused drag snaps the node to its current position and the status bar shows the reason.
- **Optimistic feel within 60 ms:** drags, typing, selection, and view changes are local and immediate. Commits, refetches, and renders run asynchronously; a failed commit rolls the local change back and says why.

## 6. Project switcher

Toolbar button with the project title. Click or `Cmd/Ctrl+O` opens a popover: a grid of cards from the workspace index (thumbnail, title, `12 scenes, 84 shots`, duration, unresolved sticky count, last author, modified time), a filter field focused on open, current project marked. Arrow keys and Enter pick. Switching commits any focused field first, drops the old project from the client, and shows the new graph from its saved viewport. The server holds only the active project.

## 7. Keyboard map

Graph shortcuts are active when focus is not in a text field. `?` shows this map as an overlay.

| Keys | Action |
|---|---|
| Space+drag, middle-drag | Pan |
| Wheel, pinch, Cmd/Ctrl+wheel | Zoom at pointer |
| `F` | Frame selection, or everything if none |
| `0` | Zoom 100% |
| Left/Right | Previous/next shot in story order |
| Up/Down | Nearest shot in the scene above/below |
| Alt+Left/Right | Reorder selected shot |
| Shift+click, Cmd/Ctrl+A | Add to selection, select all |
| `A` / Shift+A | Add shot / add scene |
| `N` | New sticky (pinned to selection if any) |
| `1` `2` `3` `4` | Status draft / review / approved / changes |
| `[` `]` | Previous/next panel |
| Enter | Edit title of selected shot (or open panel review from the preview) |
| Delete, Backspace | Remove selection |
| Shift+T | Tidy to auto-layout |
| Cmd/Ctrl+Z | Undo my last change |
| Cmd/Ctrl+O | Project switcher |
| Cmd/Ctrl+\ | Toggle inspector |
| Esc | Cancel drag or edit, close overlay, then clear selection |

## 8. Empty, loading and error states

- **Empty workspace:** centred: "No projects in `<folder>`." and the CLI line to create one (`storyboard new <name>`), with a copy button.
- **Empty project:** "No scenes yet." `Add scene` button, and the agent hint `storyboard add-scene ...`.
- **Empty scene:** frame with ghost slot (section 2.2). **Blank panel:** the renderer's visible placeholder.
- **Loading project:** nodes and frames draw from the document at once with grey thumbnails; thumbnails fill in as they arrive (visible top-left first). The toolbar title shows a small spinner until the first paint.
- **Render pending:** progress bar on the thumbnail or preview, old image kept.
- **Missing source / overflow:** the render shows it; the inspector preview lists render `warnings` in a single muted line under the image.
- **Server gone:** pill turns `Reconnecting`, then `Offline` after 10 s; the UI goes read-only with a top banner "Lost connection to storyboard. Edits are paused." Drafts are kept; reconnect resumes and refetches.
- **Refused write:** status bar or field message with `hint`; graph unchanged.

## 9. Visual direction

Calm, dense, tool-like. The canvas is pure neutral grey so panel images read true; colour is reserved for meaning (status, author, selection, cut, conflict). Type: `-apple-system, BlinkMacSystemFont, "Segoe UI", Inter, sans-serif`; mono `ui-monospace, "SF Mono", Menlo, Consolas, monospace` for numbers, ids and timing, with `font-variant-numeric: tabular-nums`. Three sizes: 11, 13, 15 px. Spacing on a 4 px grid; radii 4 px (nodes), 8 px (frames, popovers). No shadows on the canvas; one soft shadow on popovers.

```css
:root {
  --canvas: #1c1c1c;  --grid-dot: #2a2a2a;   --frame: #222222;  --frame-header: #8f8f8f;
  --surface: #242424; --surface-2: #2e2e2e;  --border: #383838; --border-strong: #555555;
  --text: #e6e6e6;    --text-dim: #a0a0a0;   --text-faint: #6e6e6e;
  --line: #9a9a9a;    --cut: #4fb8c0;        --select: #f2f2f2;
  --status-draft: #8a8a8a; --status-review: #d9a93a; --status-approved: #4fb878; --status-changes: #e0645a;
  --human: #6aa6ff;   --agent: #c08cff;      --conflict: #f08a24;
  --sticky: #2d2a22;  --sticky-text: #e9e2cc;
}
```

Author colour is the 3 px bar on stickies, the author label and the remote-change ring. Every agent shares `--agent`; the name tells them apart.

## 10. Needs not covered by any story (deferred; not in this design)

- Redo: the store has per-author undo only; `Shift+Cmd+Z` is unbound until a story exists.
- Cut editing in the graph: create, rename and delete cuts; add, remove and reorder clips; trim `in`/`out` and `slot`.
- Creating a project from the UI (today only `storyboard new`).
- Source browser in the UI: see `sources/`, attach `refs`, place an image on a panel by drag and drop.
- Layer editing in the preview (move, resize, hide, reorder); S9 shows layers read-only.
- Reordering panels in the strip (`move_panel`) and takes (`move_take`) by drag.
- Reordering scenes (`move_scene`) from the graph.
- A history view: browse the commit log, who changed what, jump to a change.
- A stills flip-through of the sequence at shot durations (distinct from the video playback non-goal; needs an owner ruling).
- Search and filter shots by text, status or unresolved stickies.
- Per-agent colours when several agents write at once.
- Screen-reader access to the graph beyond the keyboard map.
- Fallback fonts in the browser (the spec defers them to the UI plan; the renderer's bundled font covers panel images only).
