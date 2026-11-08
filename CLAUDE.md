# nb-storyboard

A local tool for art directing a story and resequencing it in a node graph, where a person and an LLM agent (Claude Code or Codex) work on the same shot at the same time through the same validated operations.

## Product identity

The user looks at a panel, says what is wrong with it, and the agent restructures the panel, the shot or the sequence on the fly. The tool is the shared workbench: both read and write one project through the same validated operations, and the screen updates as soon as either side changes something.

Goals:
1. Art direct with an agent: read any panel, change it, see it in under two seconds on a project of 30 scenes.
2. Resequence: each shot is a node, a group of shots is a scene, and a sequence is an ordered path of shots.
3. Many projects open, cheap to flip between; memory does not grow with the number of open projects.
4. Stickies: notes on the graph, optionally pinned; the notes channel between the user and the agent.
5. One human-readable, git-friendly format: a folder per project holding all its source material.
6. An executable people download and run. Mac first, then Windows.

Anti-goals: a chat box inside the app; memory that scales with open projects; an interaction over 60 ms on the main thread; a tool that needs a compiler to try.

## Spec fidelity

- The spec is `wiki/product-spec/spec.md`. Stories are in `wiki/product-spec/stories/`. No story, no build.
- The spec is append-only for agents. Scope pressure becomes a deferral. Amendments need the owner's approval.
- Commits carry `Spec: <story path>`.
- The rules in `LAWS.md` are never weakened to make a test pass.
- This repository holds no client or employer material. Keep it that way.

## Commands

- Test: `cargo test --workspace --all-features`
