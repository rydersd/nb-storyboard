# S28: Readable change summaries

Status: specced (owner approved 2026-11-10: "write the api gaps ... finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: API gaps found while building S9, 2026-11-08.

## Story

History and the status bar describe a change in words a person reads at a glance, e.g. "moved Mara waits to The Train", not op names.

## Acceptance

1. Each commit summary names the objects by title where they have one (shot, scene, cut, take), and the action in plain words, for every public op.
2. A batch summarises its parts ("reordered 3 shots") and stays one line under 120 characters.
3. Existing log records keep their stored summary; only new commits change.
4. `history`, MCP results and the `project` live event carry the new summary; tests cover each op family.
