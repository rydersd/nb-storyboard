# S4: Stickies

Status: specced

See [the specification](../spec.md).

## Story

The person and the agent leave notes on the graph, optionally pinned, ask and answer in a thread, and resolve it.

## Acceptance

1. `sticky_*` tests pass
2. A reply links to its question, resolving clears the thread, and a double resolve is refused
3. `notes --all` shows resolved notes
4. Authorship follows the Honest authorship law

Selectors are checked by requiring at least 1 passed, because a name matching nothing exits 0.
