# Task 9 fix-round walkthrough evidence

Date: 2026-11-07

Source/build identity: `redo-t9` worktree, `target/release/storyboard`, built with `cargo build --release -p storyboard-cli`. `target/release/storyboard --version` returned `storyboard 0.1.0`.

## Download and first run

A local release-shaped archive was made from that binary with `LICENSE-APACHE`, `LICENSE-MIT`, and the revised `README.md`. Its `SHA256SUMS` entry passed the exact documented verification command:

```text
shasum -a 256 -c SHA256SUMS --ignore-missing
storyboard-aarch64-apple-darwin.tar.gz: OK
```

The README now names the Releases page, commands for each Mac architecture, a fixed discoverable default workspace (`$PWD/storyboard-workspace`), the Gatekeeper quarantine removal, and the explicit `STORYBOARD` paths. Tag CI has not run in this local evidence; therefore this does not prove a published release or Gatekeeper acceptance.

## README CLI walkthrough

The runnable README block ran with `STORYBOARD` bound to the release binary and `WORKSPACE` bound to a fresh temporary parent. It created `storyboard-workspace/night-station`, added Platform and four shots (Arrival, Question, Decision, Departure), added text, rendered both images, added then undid a sticky, and displayed the four-item sequence.

```text
panel-image: 960 x 792, warnings=[]
cut-sheet: 1200 x 639, warnings=[]
undo: version restored to the post-text version
show: 4 sequence items, frames 0..192
```

## MCP setup and round trip

`cli_readme_mcp_setup_invokes_storyboard_mcp` parses the command after `--` in the Claude block and the Codex `command`/`args` fields. It proves the requested `mcp` subcommand with `"$STORYBOARD" mcp --help` and proves the typo `"$STORYBOARD" mpc --help` fails. The Codex block uses literal absolute-path placeholders because its TOML `command` field does not perform shell-variable expansion.

The rerunnable stdio client is [`scripts/mcp_round_trip.py`](../../../scripts/mcp_round_trip.py). It was run as:

```text
python3 scripts/mcp_round_trip.py target/release/storyboard <fresh-workspace>
version=d6383b42acf952ae04c950a29e036ee07615f2173d20a1f8ab482c6ea08b065d ... image_block=true
```

The client initialized MCP, created a project, scene and shot, added text, and rendered `panel_image` with integer `at: 1`; its response began with an MCP image block. `mcp_panel_image_accepts_integer_at` provides the same focused regression coverage. No real Claude Code or Codex registration was run, and no machine configuration was changed.
