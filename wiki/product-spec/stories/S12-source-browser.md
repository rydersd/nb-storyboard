# S12: Browse sources and place images

Status: specced (owner approved 2026-11-10: "finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: [workbench UX plan](../plans/workbench-ux.md), section 10, 2026-11-07.

## Story

A person sees the project's `sources/`, attaches refs to a shot or take, and drops an image onto a panel to place it as a layer.

## Acceptance

1. A source panel lists files from the sources scan with kind, size and what uses each; missing refs are flagged.
2. Drag a source onto a shot or take attaches a ref (`add_ref`); remove detaches it.
3. Drop an image file on the preview places an image layer; a file from outside the project is imported first and never overwrites unasked.
4. A new file in `sources/` appears without a reload (the `source` event).
