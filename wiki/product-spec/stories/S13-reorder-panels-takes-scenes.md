# S13: Reorder panels, takes and scenes by drag

Status: specced (owner approved 2026-11-10: "finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: [workbench UX plan](../plans/workbench-ux.md), section 10, 2026-11-07.

## Story

A person drags panels in the strip, takes in the switcher and scenes on the graph to reorder them.

## Acceptance

1. Drag in the panel strip sends `move_panel`; in the takes switcher `move_take`; a scene frame's handle `move_scene`.
2. Each reorder is one undoable commit, with an insertion indicator before release.
3. Keyboard equivalents exist for each.
