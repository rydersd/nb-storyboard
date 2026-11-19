# S22: Play and retime an audiovisual cut

Status: active implementation (MP4, shot and placement VO, cut and story-order playback, last-frame hold, placement split and safe same-shot placement merge, independent revised-shot split, labeled timeline, placement-end trim, ordinary placement-boundary rolling and guarded imported-source boundary dragging, playhead-to-inspector selection and bounded audio waveforms; remaining acceptance below)

Epic: [E2 the workbench](E2-workbench.md). This replaces the stills-only scope of [S18](S18-stills-flipthrough.md) and depends on cut editing in [S10](S10-cut-editing.md).

## Story

A person places a shot node in the player, attaches storyboard panels, video or voice-over, and sees its playback time determined by that content. They can adjust the timing while watching, and intercut events from multiple scenes. Playback shows one scene at a time, with hard cuts between them. The scenes can represent events happening at the same time.

## Confirmed behavior

1. The viewer plays, pauses, seeks and frame-steps through the active cut. The playhead and displayed panel or video frame agree with the compiled sequence frame. The viewer shows one scene at a time; a jump to another scene is a hard cut.
2. A timeline shows each placement in playback order, its scene and shot identity, attached content, panel boundaries, trims, resulting duration and audio waveform where audio exists. Selection and playhead stay in sync with the graph and inspector.
3. A placement's default playback span comes from the timing of its attached content, including voice-over and video. The person can change content starts and ends, panel timing, clip order and clip trims in the workbench, preview the resulting duration and undo each committed edit. Timing uses project frames as the authoritative unit, including at cut boundaries. A manual hold or fixed slot is an explicit edit, not the silent default that masks longer media.
4. A cut can alternate clips from different scenes, including repeated returns to a scene. This is an edit of the cut; it does not reorder the scenes or their source shots. The workbench provides cut editing and playback for this narrative intercut; exact shared scene clocks remain undecided.
5. Video and audio sources can be imported, linked to the appropriate shot or cut, saved with the project, and played in sync after seeking, trimming and changing playback position. Missing or unsupported media produces a visible, recoverable error. Audio must not silently drift from the video or playhead.
6. The interface keeps typing, selection and dragging responsive while media loads, seeks or decodes. An external project edit refreshes the timeline and viewer without silently losing a local timing edit.
7. If voice-over continues after its video ends, the viewer holds the video's last frame until the placement ends. The info panel warns that the video is shorter than the voice-over and shows the unmatched duration. The warning clears when timing edits remove the mismatch.

## Plan of work

1. Specify the media document model, source import and derived-duration rules. Define which content belongs to a reusable shot and which timing belongs to a particular placement, what owns audio (shot, clip or cut), how video competes with panel images, and how media time maps to project frames. Preserve deterministic sequence compilation and ops/undo rules. The current implementation derives default shot duration from attached content and preserves explicit placement trims and fixed slots; its remaining ownership decisions are recorded below.
2. Build media import and serving with bounded, project-contained source paths and a verified format policy. Add deterministic sequence timing tests for stills, video, audio, trims, padding and repeated cross-scene clips.
3. Build the viewer transport and seek path against compiled sequence frames. Verify hard cuts, frame stepping and audio synchronization through representative media fixtures.
4. Build the timeline and timing controls, including S10 cut and clip editing. Verify each edit through the actual workbench interaction and undo, then exercise an intercut that returns to the first scene.

## Open contract decision

“Happening at the same time” can mean narrative overlap: an editor intercuts the two scenes freely. It can also mean exact shared story time: time in each scene advances while that scene is off-screen. Exact synchronization needs scene clocks and rules for skipped or held material when the cut returns. The owner has confirmed one scene on screen at a time; this clock rule is still pending. Until it is settled, this story must not claim exact shared-time playback.

The default end rule uses the latest end among attached video, voice-over and panels; trims or a deliberate hold can change it. The owner ruled that voice-over outlasting video holds the last video frame and produces an info-panel warning. Media that crosses a hard cut still needs an explicit audio-continuation rule.

The current provisional implementation lets a cut placement carry its own voice-over, overriding the reusable shot's voice-over for that placement. Its untrimmed duration is the later of the shot's content end and that placement's voice-over end; explicit trims can shorten it. The voice-over ends at the placement's hard cut in player and export. These choices have focused model, API, browser and MP4 export checks; they still need the owner's audio-continuation ruling.

Explicit player seeks, frame steps and placement boundaries now align video and voice-over within half a project frame. Ordinary continuous playback retains a 120 ms correction tolerance to avoid decoder seek churn. An isolated packaged Chrome check showed the prior two-frame failure at 30 fps (playhead frame 2, video time 0), then video time 0.066666 seconds at frame 2 after the correction, with no page errors.

A second packaged Chrome check on an isolated copy attached a two-second WAV to the first imported shot through the workbench and confirmed the saved `source_audio` path. At frame 2 the audio and video elements both read 0.066666 seconds with readyState 4. Seeking to the next shot's frame-150 hard cut paused and cleared the VO element while video advanced to source time 5 seconds. The first two attempts did not select a shot with a scripted DOM click, leaving the Attach VO button disabled; a physical pointer click corrected that harness fault. The durable project was not edited.

A later packaged Chrome check used a 30-frame MP4 whose final frame is blue and preceding 29 frames are red, plus a two-second WAV. Seeking to held project frame 45 showed the final blue frame at source time 0.999 seconds, with the video paused and the one-second VO mismatch warning visible. The old 40 ms end clamp selected a red frame at 0.96 seconds in the same decoder. The player now clamps video and VO 1 ms inside their selected source range. A second packaged Chrome check paired a 24 fps project with a 60 fps source whose final frame alone was blue; held project frame 36 also decoded blue at 0.999 seconds with the one-second warning and no page errors. These checks cover final-frame display across equal and mixed frame rates; exact shared scene clocks and cross-cut audio still need owner rulings.

## Acceptance scenario

Import numbered images as scene A's shot and five-second video with eight-second voice-over as scene B's shot. Verify that B holds its last video frame for the final three seconds, the info panel warns about that three-second mismatch, and a timing edit that removes the mismatch clears the warning. Build a cut A → B → A, play it with hard cuts, seek backward across a cut boundary, retime a panel and trim B, then replay. The displayed frames, playhead and sound agree with the committed frame timing; undo restores the earlier timing. The story-time interpretation of the return to A follows the owner ruling above.

Same-shot placement merge is available when two adjacent clips have contiguous source trims, identical shot/take and placement VO, and no fixed slot or nested cut. The merge is one undoable edit and preserves the compiled cut length. Merging distinct revised shots still requires a separate design because their boards and media may differ.
