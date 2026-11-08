# S2: Resequence the story

Status: specced

See [the specification](../spec.md).

## Story

Each shot is a node, a group of shots is a scene, and a sequence is an ordered path. A person or agent reorders, inserts, removes, branches, swaps a take and trims, and can always see what the sequence now is.

## Acceptance

1. `compile_*` and `op_*` tests pass
2. `show` prints the sequence as a table; `cut-sheet` renders it as an image
3. Moving a shot between scenes keeps its identity and takes

Selectors are checked by requiring at least 1 passed, because a name matching nothing exits 0.
