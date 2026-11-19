# S20: Extension points

Status: specced (owner approved 2026-11-10: "finish 10-20")

Epic: [E2 the workbench](E2-workbench.md). Source: owner request "make this modular ... extension points", 2026-11-08.

## Story

Someone adds a panel, a node badge, a canvas tool or a command to the workbench as a separate module, without editing the core, and it reads and writes the project only through the same validated operations as everything else.

## Acceptance

1. A documented registry: `panels`, `nodeBadges`, `commands` (with key bindings) and `inspectorSections`; every built-in feature registers through it.
2. An extension receives a context with the current document and version, the selection, a change subscription and `commit(ops)` / `undo()`; it cannot write the document any other way.
3. An extension that throws is isolated: its panel shows the error, the rest of the workbench keeps working, and a 60 ms budget breach is reported against it.
4. A sample extension in the repository (for example, a word count of dialogue per scene) is loaded by a test and passes.
5. OWNER RULING NEEDED, loading: (a) extensions are compiled into the binary only (web/extensions/), or (b) also loaded from a workspace folder (e.g. `WORKSPACE/.storyboard/extensions/*.js`), which runs code from the workspace in the page. Default if silent: (a). Ruled by default 2026-11-10: (a), built-in only.

## Non-goals

A package registry or marketplace, extensions running outside the page, extensions in the Rust core.
