# S23: Turn a rough MP4 into editable boards

Status: MP4 import with timed samples, playhead marks or reviewable visual cut suggestions, browser playback and manual replacement implemented in workbench dirty tree; full acceptance pending real Claude execution and owner review

Epic: [E2 the workbench](E2-workbench.md). Depends on project lifecycle [S15](S15-new-project.md) and feeds the audiovisual player [S22](S22-player-timing-intercut.md).

## Story

A person imports an MP4 rough cut into a project, gets editable storyboard shots with representative frames and exact references back to the source movie, and uses those boards as the base for a revised cut.

## Acceptance

1. Import starts in the workbench. The original MP4 is copied into `sources/` without altering the supplied file. Import reports codec, dimensions, frame rate, duration and audio presence before creating boards. Unsupported or incomplete media fails with a useful error.
2. The person chooses or confirms shot boundaries. The first pass must work on a continuous movie with no hard cuts, such as the supplied 86-second `Landing Page.mp4`; hard-cut detection alone is insufficient. A timed sampling option and manual boundary adjustments remain available even when cuts are detected.
3. Each resulting shot has a representative image and a source movie time range. The source range remains attached when the shot is resequenced or retimed, so the user and Claude can identify exactly which original frames a revision refers to.
4. The imported board is editable through ordinary project operations and undo. A failed import does not leave half-created document edits. Generated images and the original movie are recoverable project sources.
5. The player can compare a selected board with its source movie range and play the revised cut. The user can record a revision on a shot or clip and pass its source range, current timing and note to Claude through a tested handoff path.

## Evidence and decisions

The read-only sample `/Users/ryders/Downloads/Landing Page.mp4` is 86 seconds, H.264/AAC, 1920×1080, 30 fps and 5,855,910 bytes. A 10-percent scene-change scan found no hard cuts; a contact sheet shows a continuous page demonstration. This is an acceptance case for a combined sampling and boundary-editing workflow, not evidence that hard-cut detection should be omitted for other movies.

The first implementation may use the installed FFmpeg tools for probing and frame extraction. Packaging must state and verify that dependency until the app bundles or replaces it. The import must use fixed argv, bounded file sizes, project-contained paths and source-bound tests; shell interpolation is not acceptable.

The workbench accepts MP4 files through 1 GB using a bounded disk stream, probes them with `ffprobe`, and offers either a configurable 0.25–60 second sampling interval or cut marks added while previewing the uploaded movie. Cut marks can also be entered in seconds, clicked to seek the preview, and removed. They are rounded to project frames and must leave at least one frame per shot. Each shot gets a midpoint PNG and the exact original movie range; the inspector can edit source in/out. A continuous 86-second sample created 18 five-second shots with interval sampling. A focused browser check on a two-second movie marked 1.5 then 0.5 seconds at the playhead, saw the marks sort, and saved three source-linked shots. The source MP4 remains in project `sources/`. The generated document edit is one undoable batch. Browser upload and video playback passed focused checks with the supplied sample. A selected shot can also take a revised MP4 while retaining its original storyboard panel; a focused Chrome check confirmed that a two-second replacement made the shot two seconds long. The Claude brief includes the absolute project path and source ranges. CLI and MCP `replace_movie` now probe and copy an agent-produced MP4 and attach it through an undoable edit; focused CLI and MCP checks passed. The workbench now offers optional, reviewable FFmpeg scene-change suggestions: a two-color hard-cut fixture produced one mark and two source-linked shots, while the Landing Page sample produced no marks at a 1-percent threshold. An FFmpeg score scan showed its transition peaks below 0.8 percent; the revised 0.5-percent default produced 16 reviewable marks after nearby-frame consolidation, without editing the project. Actual Claude MCP execution remains unverified. Guarded source boundary rolling passed a focused browser check for adjacent untouched imported shots.

The inspector now lets a selected panel replace its full-canvas board image with a copied project source. A focused packaged-browser check on a copy of the supplied 18-shot project replaced its first imported frame, saw the red replacement in the preview and thumbnail, read the saved layer source through the API, and used one Undo to restore the original frame while keeping the 2,580-frame cut. The original movie source range remained attached.

The selected panel's Layers section also adds, edits and removes text directly on its board through undoable operations. A packaged-browser check on an isolated copy of the MP4 project saved each state, observed a different rendered panel PNG with text, and used one Undo to restore the image-only panel.

## Open decisions

- Initial division: timed samples, manual marks, or cut detection plus timed fallback. The owner has been asked.
- The first Claude output: revised boards/notes, replacement clips or a complete movie. The owner has been asked.
- Representative frame policy and boundary snapping for variable-frame-rate sources need to be specified before promising frame-exact round trips.

Interval marks are rounded to project frames before extraction. A trailing interval shorter than one project frame is folded into the preceding shot; the workbench reports the saved shot count. A real two-second MP4 sampled every 0.99 seconds passed HTTP and Chrome checks as two one-second shots, after the old extraction failed at 1.99 seconds.

The first browser-playable import format is H.264 video with AAC audio (or no audio) in MP4. Upload now rejects other codec combinations with explicit H.264/AAC guidance and removes the rejected source copy. A real MPEG-4 video MP4 had uploaded but failed Chrome preview; a focused server regression now rejects that file and an H.264/AC-3 file. An H.264 clip loaded to readyState 4 in Chrome and imported into two editable shots.
