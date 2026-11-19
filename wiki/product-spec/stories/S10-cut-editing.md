# S10: Edit cuts in the graph

Status: initial cut editor, physical list drag and graph bead drag verified in isolated Chrome; agent-authored cut rename appeared live in the path picker; focused bead drags measured 16.7 ms p95 at 18 shots and 16.9 ms p95 at 200 shots to the second animation frame, while the every-interaction latency claim remains open

Epic: [E2 the workbench](E2-workbench.md). Source: [workbench UX plan](../plans/workbench-ux.md), section 10, 2026-11-07.

## Story

A person creates, renames and deletes cuts, and builds one by adding, removing and reordering clips on the graph, with trim (`in`, `out`) and fixed `slot`, so resequencing an alternative path never needs the CLI.

## Acceptance

1. Create, rename and delete a cut from the path picker; delete refuses with the server's message when another cut nests it.
2. Add a shot to the viewed cut, remove a clip, and drag a clip bead to reorder it; each is one undoable commit.
3. Trim and slot edit in place on a selected clip and the sequence timing updates.
4. An agent's cut edit appears live, as in S9.
5. Every interaction responds within 60 ms.
