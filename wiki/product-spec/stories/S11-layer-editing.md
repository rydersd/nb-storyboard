# S11: Edit layers on the panel preview

Status: specced (owner approved 2026-11-10: "finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: [workbench UX plan](../plans/workbench-ux.md), section 10, 2026-11-07.

## Story

A person selects a layer on the large preview and moves, resizes, hides, reorders or retypes its text, so a small art-direction fix does not need the agent.

## Acceptance

1. Click a layer in the preview or the layer list to select it; handles show its bounds.
2. Drag moves, handles resize; the commit on release is one `update_layer`.
3. Hide, opacity and order (`move_layer`) from the layer list; text edits in place.
4. The rendered preview refreshes within two seconds on the 30-scene fixture.
5. Every interaction responds within 60 ms (handles move locally; the render follows).
