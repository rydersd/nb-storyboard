# S9: The workbench interface

Status: specced (owner approved 2026-11-07)

See [the specification](../spec.md), section "Deferred to the UI plan".

## Story

A person runs `storyboard` and works in a window: the shots of a story laid out as a node graph, a selected shot's panels shown large, stickies on the graph. When an agent (Claude Code or Codex) changes the project through the CLI or MCP, the window shows it without a reload. Every change the person makes goes through the same validated operations the agent uses.

## Acceptance

1. `storyboard serve [workspace]` starts a local server bound to 127.0.0.1 only and opens the browser; the page and its assets are embedded in the one executable (no Node, no build step).
2. Graph: shots are nodes inside scene frames; story order is drawn as a path; cuts are alternate paths; dragging a node saves its position; dragging along the path reorders shots (MoveShot) and is undoable.
3. Shot view: selecting a shot shows its active take's panels rendered, the layer list, text, timing and notes; text and timing edit in place; a new take or panel can be added.
4. Live: a write by the agent through the CLI or MCP appears in an open window in under two seconds, without a reload, on the 30-scene fixture.
5. Stickies: add, reply, resolve and pin to a shot, take or panel from the graph; the agent's stickies show who wrote them.
6. Projects: a switcher lists the workspace index (title, thumbnail, counts); switching projects does not grow memory with the number opened.
7. Conflicts: if the agent changed the same thing first, the window shows the conflict and the current state instead of overwriting.
8. Every click, drag and keystroke responds within 60 ms; renders and loads run off the interaction path with a visible in-progress state.

## Non-goals

A chat box (the agent stays in its own terminal), drawing or painting tools, video playback, Windows packaging (S7 phase 2).
