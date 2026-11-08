# S6: Laws

Status: specced

See [the specification](../spec.md).

## Story

The rules in LAWS.md are tested on generated projects and edits, so a change that breaks a rule fails the build.

## Acceptance

1. `cargo test --workspace --all-features law_` reports at least 10 passed
2. Each law is shown able to fail by breaking the rule on purpose, recorded in evidence
3. Generators cover every op, nested batches, and human and agent authors
4. CI runs them on every push

Selectors are checked by requiring at least 1 passed, because a name matching nothing exits 0.
