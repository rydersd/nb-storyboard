# S5: Source material

Status: specced

See [the specification](../spec.md).

## Story

A user gathers references, stills, scripts, audio, video and notes in one project folder by dropping files in. The tool and the agent can see what is there, attach it to shots and takes, and place images on panels. Moving the folder moves the whole project.

## Acceptance

1. `sources_*` tests pass
2. A dropped file appears with no commit, including in subfolders
3. `import` never overwrites unasked and never leaves the project
4. A gone ref is reported by name and does not stop rendering

Selectors are checked by requiring at least 1 passed, because a name matching nothing exits 0.
