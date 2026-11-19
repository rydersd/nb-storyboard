# S14: See the change history

Status: specced (owner approved 2026-11-10: "finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: [workbench UX plan](../plans/workbench-ux.md), section 10, 2026-11-07.

## Story

A person opens a history panel to see who changed what and when, and jumps to the changed shot.

## Acceptance

1. The panel lists commits newest first with author, time and summary (`/api/history`).
2. Selecting a commit selects and reveals the shot it touched.
3. Agent and human commits are told apart at a glance.
4. Opening the panel never blocks the graph (loads off the interaction path).
