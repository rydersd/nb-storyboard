# S8: Usable surface

Status: specced

See [the specification](../spec.md).

## Story

A person builds a four-shot story from `--help` and the README alone, without writing JSON. An agent does the same over MCP, and can see what it made.

## Acceptance

1. `cli_*` and `mcp_*` tests pass
2. Every flag has help; `schema` prints every op and layer shape
3. Errors carry `hint`, and `valid` where there are options, with all issues at once
4. `undo` works on CLI and MCP
5. `panel_image` returns an image block
6. A walkthrough test runs the README commands as written

Selectors are checked by requiring at least 1 passed, because a name matching nothing exits 0.
