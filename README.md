# nb-storyboard

nb-storyboard is a local Rust tool for creating and editing versioned storyboard projects. Its CLI emits JSON, so people, Claude Code, and Codex can use the same project operations.

The node graph UI and server are planned; they are not built yet. Today you work through the CLI, MCP and rendered frames.

## Project layout

A project is a folder. `storyboard new` creates this layout:

```
my-project/
  .lock         persistent advisory lock file
  project.json  storyboard document
  assets/       images and other project media
  history/      prior project revisions and commit log
  .cache/       generated snapshots, thumbnails, index, and resized images
```

`project.json` contains project settings, scenes, takes, cuts, stickies, and layout. Operations update it transactionally and record history.

### Take text

| TextFields field | Meaning |
| --- | --- |
| `voice` | Spoken line |
| `action` | What is seen |
| `production` | Production notes |
| `target` | Intended outcome |
| `continuity` | Continuity notes |
| `speaker` | Speaker |
| `persona` | On-screen persona |
| `status` | Review or production status |

## CLI

From this repository, create and inspect a project:

```sh
PROJECT="$(mktemp -d)/my-storyboard"
cargo run -p storyboard-cli -- new "$PROJECT" --title "My storyboard"
cargo run -p storyboard-cli -- describe-project "$PROJECT"
```

Use `cargo run -p storyboard-cli -- --help` to see the JSON CLI operations, including reads, changes, history, frame rendering, and sticky notes.

History and conflict responses use compact operation summaries by default. To inspect complete commit records, including inverse operations, use `storyboard history <project-dir> --full` on the CLI or pass `{"full": true}` to the MCP `history` tool. Conflict responses remain compact; fetch full records with `history` when needed.

## Claude Code and Codex

The MCP server uses stdio and is configured per invocation, without permanent machine registration. With the project above and a debug binary built by the CLI command, use these commands:

```sh
BIN="$(pwd)/target/debug/storyboard"
WORKSPACE="$(dirname "$PROJECT")"
MCP_CONFIG_DIR="$(mktemp -d)"
printf '{"mcpServers":{"storyboard":{"command":"%s","args":["mcp","%s"]}}}\n' "$BIN" "$WORKSPACE" > "$MCP_CONFIG_DIR/claude-mcp.json"
claude -p --mcp-config "$MCP_CONFIG_DIR/claude-mcp.json" --strict-mcp-config --allowedTools mcp__storyboard__describe_project -- 'Describe the my-storyboard project.'
codex exec -c "mcp_servers.storyboard.command=\"$BIN\"" -c "mcp_servers.storyboard.args=[\"mcp\",\"$WORKSPACE\"]" 'Describe the my-storyboard project using the storyboard MCP server.' < /dev/null
```

Permanent `claude mcp add` registration is optional.

## Test

```sh
cargo test --workspace --all-features
```

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option. The bundled Noto Sans font is under the [SIL Open Font License](crates/storyboard-core/src/render/OFL.txt).
