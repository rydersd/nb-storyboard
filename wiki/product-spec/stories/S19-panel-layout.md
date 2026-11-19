# S19: Arrange the workbench in panels

Status: specced (owner approved 2026-11-10: "finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: owner request "make this modular", 2026-11-08.

## Story

A person arranges the workbench as panels: the graph, the shot inspector, notes, history and any added panel can be shown, hidden, resized and docked left, right or bottom, and the arrangement is remembered per workspace.

## Acceptance

1. Every built-in view (graph, inspector, notes, project switcher) is a panel registered through the same registry an extension uses (S20).
2. Show and hide each panel from a View menu and its shortcut; drag a panel's tab to dock it left, right or bottom; drag a splitter to resize.
3. The arrangement persists per workspace in `.cache/` and is restored on reload; a reset returns to the default layout.
4. The graph keeps its 60 ms interaction budget with every panel open; hidden panels do no work.
5. Live updates (S9 #4) reach every open panel; a closed panel catches up when opened.

## Non-goals

Floating or detached windows, multiple graph views of one project.
