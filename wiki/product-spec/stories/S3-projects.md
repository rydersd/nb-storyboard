# S3: Flip between projects

Status: specced

See [the specification](../spec.md).

## Story

Many projects sit in a workspace folder and flipping between them is cheap; memory does not grow with the number of projects.

## Acceptance

1. `workspace_*` tests pass
2. A warm index reads no documents
3. Only the active project is held in memory

Selectors are checked by requiring at least 1 passed, because a name matching nothing exits 0.
