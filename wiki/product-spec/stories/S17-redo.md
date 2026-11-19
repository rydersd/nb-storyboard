# S17: Redo

Status: specced (owner approved 2026-11-10: "finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: [workbench UX plan](../plans/workbench-ux.md), section 10, 2026-11-07.

## Story

A person who undid too far can redo.

## Acceptance

1. Shift+Cmd/Ctrl+Z re-applies the person's most recently undone commit.
2. Redo is per author, like undo, and follows the same rebase and conflict rules.
3. Needs a store-level design (redo is not in the spec's operations); the spec amendment comes first.
