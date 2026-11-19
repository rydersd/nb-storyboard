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

## Open the workbench

The workbench is the browser interface: the story as a node graph, the selected shot's panels shown large, and stickies on the graph. The page is built into the same binary. MP4 import accepts H.264 video with AAC audio (or silent video) and requires `ffmpeg` and `ffprobe` on your PATH. Other MP4 codecs receive a conversion error before they can become unplayable project sources.

On macOS, `scripts/package-workbench.sh` builds a local release folder at `dist/NB Storyboard`. Double-click **Start NB Storyboard.command** there to create or reopen `~/Documents/NB Storyboard Projects` and open the workbench. Keep its Terminal window open while editing; Ctrl+C stops the server. The folder contains the binary and a SHA-256 manifest, and can be moved together. Repackaging replaces the binary atomically, so a running server keeps its old build until restarted. The launcher checks `/opt/homebrew/bin` and `/usr/local/bin` as well as the inherited PATH for FFmpeg/FFprobe, and adds `~/.local/bin` for the Claude CLI. FFmpeg and FFprobe must be installed on the launching Mac; Claude CLI/login is needed only for **Run Claude**. This local package is unsigned.

```sh
"$STORYBOARD" serve "$WORKSPACE"
```

It listens on `127.0.0.1:7420` only (`--port` to change it, `--no-open` to skip opening the browser). Your edits land through the same validated operations the CLI and MCP use, as author `human`. When an agent writes to the same project, the open page updates without a reload. Stop it with Ctrl+C.

Use the project button to create a project, close the current one, or open another. Edits save to the project as they are committed; closing waits for pending field edits. A closed project stays closed after reloading the page until you choose one to open.

Select a panel to replace its board image. Open **Layers** to add text directly on the board, edit that text, or remove its text layer; these edits join the project's Undo history. Panel direction remains a separate note for staging instructions.

To try it on sample material, `scripts/workbench-demo.sh /tmp/storyboard-demo "$STORYBOARD"` builds a small workspace with two projects.

To turn numbered images into one shot, click **Import images** in the workbench, choose a scene and duration, then import. The CLI offers the same operation:

```sh
"$STORYBOARD" import-shot "$PROJECT" --base-version "$VERSION" --author human \
  --scene scene-1 --title "Arrival" frame-1.png frame-2.png frame-10.png
```

The files are copied into the project's `sources/` folder. Their numeric order determines panel order; the default shot duration is one second per image.

To revise a still, select its shot and panel, then click **Replace board image** in the inspector. The image is copied into project sources; the original movie range and other panel layers stay attached. Cmd/Ctrl+Z restores the previous board image.

To turn a rough MP4 into boards, click **Import MP4**, choose a file up to 1 GB, and review its media details. Play or seek the uploaded movie, then click **Add cut at playhead** to mark a boundary at the nearest project frame. You can also enter cut times in seconds, ask **Suggest cuts** for reviewable visual scene-change marks, click a mark to seek the preview, or remove marks. Leave the marks blank to sample at a fixed interval; boundaries snap to project frames, so a subframe tail joins the preceding shot. The importer copies the movie into project `sources/`, extracts a midpoint image for each shot, and records the original source in/out. The inspector lets you change those source bounds. **Edit cut** creates a cut, adds shots from any scene, reorders clips and adjusts trims in project frames. **Play cut** follows compiled shot order, shows audio waveforms and can show boards or the imported movie with its audio. **Export MP4** renders the selected cut using the current playback image mode and downloads H.264/AAC at the project's dimensions and frame rate. The saved render also stays in the project's `.cache/exports/` folder. Drag a placement's right-edge handle left to shorten it or right to restore unused source time; later clips ripple. Arrow keys on the handle adjust one frame. **Split placement** at the playhead makes two independently movable and trimmable placements of the same source shot. At their shared boundary, **Merge placements** joins adjacent contiguous parts of that same shot when their audio and take settings match; notes pinned to the removed part follow the merged placement, and Undo restores both parts. Unknown metadata on the removed part blocks the merge. **Split into shots** creates two distinct editable shots. For an untouched imported movie shot, it also splits the source ranges and extracts new representative frames. For a revised, reused or trimmed shot, it forks the shot with independent take, panel and layer IDs while preserving shared source files and media ranges; each new placement shows its own part of the timeline. Fixed slots, nested cuts and explicit take overrides cannot be split this way. Select a shot and click **Replace clip** to attach a revised MP4 while keeping its storyboard panels as the original edit reference. **Attach VO** copies an MP3, WAV or M4A into the project; the shot extends to the attached audio's duration, and its inspector offers audio in/out and detach controls. In **Play cut**, **Attach placement VO** copies audio onto only the selected cut placement, leaving other uses of the shot unchanged. **Set VO range** adjusts its in/out, and **Remove placement VO** restores shot-level audio. The placement runs to the later of its shot content or its own VO unless trimmed. When VO outlasts the movie range within the placement, playback holds the final video frame and warns in the player. The current cut-local VO behavior stops audio at a hard cut; cross-cut continuation awaits an owner decision. A clip's explicit **Slot** still pads with black beyond the shot duration. For two adjacent untouched shots from the same imported movie, drag the green source-boundary handle to roll their shared edge; both board frames refresh and the cut length stays fixed. Between adjacent ordinary shot placements, the boundary handle instead adjusts both clip trims without changing the source shots or cut length. Undo restores either edit. At a boundary between two different shot placements, **Group placements** makes one movable nested cut while retaining both shots, their trims, VO, notes and source media. **Ungroup** restores the two placements; Undo also restores the prior state. This is a lossless group, so the two shots remain separately editable. Direct Claude media generation is still in development.

`storyboard show "$PROJECT"` and the MCP `show` tool include source movie ranges, shot action and notes for an agent to read before making project edits. Claude can import numbered image boards with MCP `import_shot`. Claude can render an H.264/AAC MP4 from the current boards or source footage with MCP `export_movie` (`project`, optional `cut`, `mode: boards` or `original`); it returns the saved file path and project version. When Claude has generated separate footage, MCP `replace_movie` probes it, copies it into project sources, and attaches it to a shot through an undoable edit. The equivalent CLI command is `storyboard replace-movie "$PROJECT" --base-version "$VERSION" --author agent:claude --shot SHOT_ID --source generated.mp4`.

The workbench's **Run Claude** button starts one local, 15-minute Claude CLI job against the open project. Enter an instruction, then review its status and result in the same panel; **Stop run** cancels Claude and its inherited storyboard tool process. The job uses a storyboard MCP server scoped to that project and requires a working `claude` login. Its result panel checks the saved project version and lists added, edited or removed shots and cuts plus changed movie attachments, so a successful Claude message without an edit is shown as no saved project edits. A completed run lists any MP4 exports added during that run and offers download links. It can assemble existing boards or source footage into a movie, but new footage still needs an external generator; a generated MP4 can then be attached with `replace_movie` or **Replace clip**.

MP4 import offers **Suggest cuts** after upload. It scans for visual scene changes with an adjustable 0.1–50% threshold (0.5% by default), collapses candidate frames within a quarter-second transition window, rounds marks to project frames, and puts them in the editable cut-mark list for review. If no visual cuts are detected, use manual playhead marks or the interval field. Detection never changes the project until **Create boards** is selected.

**Copy Claude brief** copies the selected shots (or all shots when none are selected), the absolute workspace, project and running CLI paths, original MP4, shot and placement audio ranges, panel image sources, directions and text overlays, revision notes and cut order. Paste it into a Claude Code session connected to that storyboard MCP workspace, or use the listed CLI path to inspect the project. Claude can revise project metadata and attach a generated clip through validated operations. **Run Claude** launches a storyboard edit session from the workbench; generating a new clip still requires a separate workflow.

Use `move-shot-to-scene` to move a shot between scenes and `reorder-shot` to change its position within one scene. Cuts use the parallel `reorder-clip` verb. Run `storyboard <verb> --help` for the required ids and base version.

## Connect an agent over MCP

The server uses standard input and output. It is not registered automatically; choose a workspace whose children are storyboard projects. Its `show` tool returns the cut sequence, cut placements with trims and placement VO ranges, and each take's panel IDs, times, titles, direction, image/text layers and stroke count, so an agent can inspect the boards before editing them.

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
