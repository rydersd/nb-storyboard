# Redo implementation plan

> For agentic workers: use superpowers:subagent-driven-development. Steps use checkbox syntax.

**Goal:** rebuild the project on [the specification](../spec.md) so a person or agent can make a storyboard from a downloaded binary without writing JSON.

**Architecture:** four-layer Rust workspace: core (model, validate, compile, ops, describe, render), store (folders, commit, history, sources, workspace, watch), CLI and MCP over one operation layer.

**Tech stack:** Rust 2024, serde, clap, rmcp, notify, tiny-skia, resvg, cosmic-text, image, proptest.

**Spec:** [wiki/product-spec/spec.md](../spec.md), [LAWS.md](../../../LAWS.md)

## Global constraints

- Written from the spec only. Modules being replaced are deleted first. Do not read earlier versions of them, and do not read anything outside this repository.
- Every write goes through ops and commit. CLI and MCP never accept a whole document.
- Nothing opens a socket.
- Binary stays `storyboard`; crates stay `storyboard-core`, `storyboard-store`, `storyboard-cli`.
- Each task keeps `cargo test --workspace --all-features`, clippy with `-D warnings`, and `cargo fmt --check` green for the crates that are in the workspace at that point.

## Review focus

1. A shot in several cuts: removing it is refused and names the cuts; a sticky pinned to a clip of it survives a cut edit.
2. `selected_take` pointing at a removed take: removing the selected take reselects deterministically and inverts exactly.
3. Moving a folder or renaming it with a warm cache: paths still resolve.
4. A stale commit with an invalid value: the validation error shows, not a conflict.
5. A hand edit of `project.json` between two commits: later rebases still work, and the edit is in history.

## Tasks

### Task 1: Model, ids and validate
Files: `crates/storyboard-core/src/{ids,model,validate,lib}.rs`, tests, root `Cargo.toml`.
Start: delete `model`, `validate`, `compile`, `ops`, `footprint`, `describe` and their tests; set workspace members to core only; gate render out until Task 4.
Build the document types with stable field order, unknown-field preservation on every object, sparse writing, and the spec's validate. Named tests: `model_round_trips_fixture_exactly`, `model_keeps_unknown_fields_everywhere`, `model_writes_id_first_and_omits_empty`, `validate_reports_all_issues_at_once`, `validate_names_valid_options`, plus one per limit. Fixture: `tests/fixtures/small.json` rewritten in the new format (two scenes, four shots, two takes on one shot, one cut, one nested cut, stickies).
Spec: S1, S2.

### Task 2: Compile
Files: `compile.rs`, tests. `compile_story(&Project)`, `compile_cut(&Project, &str)`, `compile_active`, `sample(&Compiled, frame)`. Named tests: `compile_story_order_uses_selected_takes`, `compile_cut_trims`, `compile_slot_pads_and_cuts`, `compile_nested_expands_in_place`, `compile_cycle_is_error`, `compile_sample_each_frame`. Validate wires compile limits.
Spec: S2.

### Task 3: Ops
Files: `ops.rs`, `footprint.rs`, tests. Every op in the spec with exact inverses, defaults, short ids, patch field checks, narrow footprints. `apply(&mut Project, &Op) -> Result<Applied, OpError>`. Named tests: `op_<name>_inverts` for each op, `op_patch_rejects_unknown_field`, `op_new_shot_has_take_and_panel`, `op_remove_shot_in_use_names_cuts`, `op_remove_selected_take_reselects`, `op_remove_unpins_and_reports`, `footprint_remove_touches_only_pinned_stickies`, `op_move_shot_keeps_identity`.
Spec: S2, S4.

### Task 4: Describe and render
Files: `describe.rs`, `render/*`, tests. Adapt the rendering engine to panels. `describe_project`, `describe_sequence`, `describe_panel`. `render_panel(.., mode)`, `render_cut_sheet`, returning warnings. Bold, strokes, scaled lines, placeholder, missing-source box, caption strip, GIF and TIFF decoding, better downscale. Named tests: `describe_panel_*`, `render_empty_panel_is_visible`, `render_missing_source_keeps_the_rest`, `render_bold_differs_from_regular`, `render_strokes_draw`, `render_board_has_caption_strip`, `render_cut_sheet_shows_each_shot`, `render_warns_on_overflow`.
Spec: S1.

### Task 5: Store
Files: `crates/storyboard-store/*`. Adapt transactions to the spec: scaffolded `.gitignore`, log with notes and summaries, interval snapshots, external edit logging, validation before conflict, conflict reason, undo, sources scan and import, recursive watch, workspace index fields, content-keyed cache with pruning. Keep existing crash-safety behaviour and its tests where they still apply. Named tests: `commit_*`, `history_*`, `sources_*`, `workspace_*`.
Spec: S3, S5.

### Task 6: CLI
Files: `crates/storyboard-cli/src/{args,commands,ops_api}`. The spec's commands, help on every flag, `--pretty`, `--at`, `NB_AUTHOR`, errors with `hint` and `valid`. Named tests: `cli_*`, including `cli_walkthrough_builds_four_shots_without_json`.
Spec: S8.

### Task 7: MCP
Files: `crates/storyboard-cli/src/mcp`. Mirror the CLI, hide inverse ops, working agreement, image block, compact writes, author default from the client, per-type layer schema. Named tests: `mcp_*`.
Spec: S8, S4.

### Task 8: Laws
Files: `crates/storyboard-cli/tests/laws.rs` or per crate, `proptest` dev dependency, CI. The ten `law_*` tests, and evidence that each can fail. Defects found get fixed in this task.
Spec: S6.

### Task 9: README and release
Files: `README.md`, `.github/workflows/release.yml`, evidence. Rewrite the README to start with download and run, a concept map and a walkthrough. Release workflow on tags for `aarch64-apple-darwin` and `x86_64-apple-darwin` with checksums. No signing credentials are created. Run the plan's whole walkthrough with the release binary, and the MCP round trip with Claude and Codex.
Spec: S7, S8.
