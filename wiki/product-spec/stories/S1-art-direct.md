# S1: Art direct a panel

Status: specced

See [the specification](../spec.md).

## Story

A person or agent reads any panel with its layers, text, timing and notes, changes it, and sees the rendered result in under two seconds on a project of 30 scenes.

## Acceptance

1. `describe_panel_*` and `render_*` tests pass
2. Edit plus render stays under the budget on the 30-scene fixture, measured in release, recorded in evidence
3. An empty panel, a missing source and overflowing text each produce a visible result and a warning

Selectors are checked by requiring at least 1 passed, because a name matching nothing exits 0.
