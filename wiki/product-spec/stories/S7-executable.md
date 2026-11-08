# S7: Executable

Status: specced

See [the specification](../spec.md).

## Story

Someone with no developer tools downloads nb-storyboard and runs it. Mac first, then Windows.

## Acceptance

1. A tagged release publishes Mac builds for Apple Silicon and Intel with checksums, through CI
2. The download runs on a Mac with no Rust installed
3. The README starts with download and run
4. Signing status is recorded per release; an unsigned build never counts as passing Gatekeeper
5. Windows x64 is a second phase

Selectors are checked by requiring at least 1 passed, because a name matching nothing exits 0.
