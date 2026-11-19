# S15: Create a project from the workbench

Status: implemented with focused browser and relocated-package lifecycle checks; owner acceptance open (2026-11-09)

Epic: [E2 the workbench](E2-workbench.md). Source: [workbench UX plan](../plans/workbench-ux.md), section 10, 2026-11-07.

## Story

A person creates a new project from the project switcher without the CLI.

## Acceptance

1. The switcher offers New project: title, fps and format, with the same defaults and validation as `storyboard new`.
2. The project is created inside the served workspace only; names that collide are refused with a hint.
3. It opens on creation and appears in the index.
4. The switcher can close the current project and open another. Closing or switching flushes pending edits; a visible failed inspector edit prevents the change so it is not lost. Closing persists across a page reload until the person opens a project.
5. Committed edits are saved to disk as they happen. A project reopened after closing shows the latest committed document. The switcher says that edits save automatically.
