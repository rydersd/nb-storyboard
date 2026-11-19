# S26: Project lengths in time

Status: specced (owner approved 2026-11-10: "write the api gaps ... finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: API gaps found while building S9, 2026-11-08.

## Story

The project switcher shows each project's length in seconds and minutes, not frames.

## Acceptance

1. The workspace index entry carries the project's `fps`, read from the cached index like the other fields, never by opening whole documents.
2. `GET /api/workspace` returns it; the switcher shows duration as m:ss.
3. A cache written before this change is treated as a miss and rebuilt, not misread.
4. CLI/MCP index output gains the same field; existing fields are unchanged.
