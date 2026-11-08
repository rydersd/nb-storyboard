# nb-storyboard

## Download and run

Get the current archive from the [Releases page](https://github.com/rydersd/nb-storyboard/releases): `storyboard-aarch64-apple-darwin.tar.gz` for Apple Silicon or `storyboard-x86_64-apple-darwin.tar.gz` for Intel. The commands below use macOS's built-in `curl`, `shasum`, and `tar`; Rust and other developer tools are not required.

Apple Silicon:

```sh
curl -LO https://github.com/rydersd/nb-storyboard/releases/latest/download/storyboard-aarch64-apple-darwin.tar.gz
curl -LO https://github.com/rydersd/nb-storyboard/releases/latest/download/SHA256SUMS
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf storyboard-aarch64-apple-darwin.tar.gz
STORYBOARD="$PWD/storyboard-aarch64-apple-darwin/storyboard"
```

Intel:

```sh
curl -LO https://github.com/rydersd/nb-storyboard/releases/latest/download/storyboard-x86_64-apple-darwin.tar.gz
curl -LO https://github.com/rydersd/nb-storyboard/releases/latest/download/SHA256SUMS
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf storyboard-x86_64-apple-darwin.tar.gz
STORYBOARD="$PWD/storyboard-x86_64-apple-darwin/storyboard"
```

The release binaries are unsigned and not notarized. Gatekeeper will block the first run; remove the quarantine attribute, then check the binary:

```sh
xattr -d com.apple.quarantine "$STORYBOARD" 2>/dev/null || true  # only browser downloads are quarantined
"$STORYBOARD" --version
```

Keep using `"$STORYBOARD"` below, or put the binary on your `PATH` (for example, copy it into a directory already on your `PATH`).

## Concept map

```text
Project
|- Scenes: ordered named groups of shots
|  `- Shots: graph nodes with story text, duration, and takes
|     `- Takes: alternate versions, each with an ordered strip of panels
|        `- Panels: storyboard images made of layers and strokes
|           `- Layers: image, text, solid, or box
|- Cuts: named alternative orderings; clips choose a shot/take and trim
|- Stickies: notes on the graph or pinned to an object
`- Sources: files gathered under sources/ and referenced by shots or takes
```

A shot remains one node even when several cuts use it. A panel is an image; a frame is only a unit of time.

## CLI walkthrough

Set `STORYBOARD` to the unpacked binary and run this from any writable folder. The commands make a four-shot project, render a panel and the story contact sheet, add a sticky, and undo that sticky. This is an executable example: the test suite runs every block marked `runnable`.

```sh runnable
WORKSPACE="${WORKSPACE:-$PWD/storyboard-workspace}"
mkdir -p "$WORKSPACE"
PROJECT="$WORKSPACE/night-station"
version() { sed -n 's/.*"version":"\([^"]*\)".*/\1/p' "$1"; }

"$STORYBOARD" new "$PROJECT" --title "Night Station" > "$WORKSPACE/new.json"
VERSION="$(version "$WORKSPACE/new.json")"
"$STORYBOARD" add-scene "$PROJECT" --base-version "$VERSION" --author human --title "Platform" > "$WORKSPACE/scene.json"
VERSION="$(version "$WORKSPACE/scene.json")"

"$STORYBOARD" add-shot "$PROJECT" --base-version "$VERSION" --author human --scene scene-1 --title "Arrival" > "$WORKSPACE/shot-1.json"
VERSION="$(version "$WORKSPACE/shot-1.json")"
"$STORYBOARD" add-shot "$PROJECT" --base-version "$VERSION" --author human --scene scene-1 --title "Question" > "$WORKSPACE/shot-2.json"
VERSION="$(version "$WORKSPACE/shot-2.json")"
"$STORYBOARD" add-shot "$PROJECT" --base-version "$VERSION" --author human --scene scene-1 --title "Decision" > "$WORKSPACE/shot-3.json"
VERSION="$(version "$WORKSPACE/shot-3.json")"
"$STORYBOARD" add-shot "$PROJECT" --base-version "$VERSION" --author human --scene scene-1 --title "Departure" > "$WORKSPACE/shot-4.json"
VERSION="$(version "$WORKSPACE/shot-4.json")"

"$STORYBOARD" add-text "$PROJECT" --base-version "$VERSION" --author human --at 1 --text "A train enters the empty station." > "$WORKSPACE/text.json"
VERSION="$(version "$WORKSPACE/text.json")"
"$STORYBOARD" panel-image "$PROJECT" --at 1 --mode board --out "$PROJECT/.cache/panel.png"
"$STORYBOARD" cut-sheet "$PROJECT" --out "$PROJECT/.cache/cut-sheet.png"

"$STORYBOARD" note "$PROJECT" --base-version "$VERSION" --author human --kind todo --pin shot:shot-1 --text "Check the platform sign." > "$WORKSPACE/sticky.json"
VERSION="$(version "$WORKSPACE/sticky.json")"
"$STORYBOARD" undo "$PROJECT" --base-version "$VERSION" --author human
"$STORYBOARD" show "$PROJECT" --pretty
```

The render commands write `panel.png` and `cut-sheet.png` under the project's disposable `.cache/` directory. `show --pretty` prints the resulting sequence rather than requiring a JSON editor.

Use `move-shot-to-scene` to move a shot between scenes and `reorder-shot` to change its position within one scene. Cuts use the parallel `reorder-clip` verb. Run `storyboard <verb> --help` for the required ids and base version.

## Connect an agent over MCP

The server uses standard input and output. It is not registered automatically; choose a workspace whose children are storyboard projects.

For Claude Code, register the downloaded binary when you decide that machine should use it:

```sh
claude mcp add storyboard -- "$STORYBOARD" mcp "$WORKSPACE"
```

For Codex, add the equivalent entry to its `config.toml`. Replace the two placeholder paths with the absolute values of `"$STORYBOARD"` and `"$WORKSPACE"`; TOML does not expand shell variables:

```toml
[mcp_servers.storyboard]
command = "/absolute/path/to/storyboard-aarch64-apple-darwin/storyboard"
args = ["mcp", "/absolute/path/to/storyboard-workspace"]
```

Both clients can read with `show`, `describe` (a panel's layers, text and timing), `notes`, `history`, and `schema`; writes use the `base_version` returned by the previous read or write. Stickies are the shared note channel. The walkthrough's MCP evidence exercises the server directly over stdio, but no real Claude Code or Codex registration has run.

Law 7, Honest authorship, is enforced at the MCP boundary: an MCP client can create stickies only under its own client identity. The local CLI deliberately trusts the explicit `--author` value (or `NB_AUTHOR`), because the person invoking a local process controls that identity.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option. The bundled Noto Sans font is under the [SIL Open Font License](crates/storyboard-core/src/render/OFL.txt).
