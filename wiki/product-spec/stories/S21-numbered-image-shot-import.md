# S21: Import numbered images as a shot

Status: implemented with focused CLI, server, browser and preflight checks (2026-11-09); owner acceptance open

## Story

A person selects a set of numbered image files and gets one new shot. The images become its active take's panels in natural numeric order, one full-frame image layer per panel. The original files are copied into the project's `sources/` folder. The person can start this from the workbench or the CLI.

## Acceptance

1. The workbench offers an import action for multiple image files; the CLI accepts multiple paths. The user chooses the owning scene, shot title and duration. The default duration is one second per image. A new shot is inserted after the selected shot in that scene, or at the scene end.
2. Numbered filenames sort by numeric value (`frame-2` before `frame-10`), with a stable case-insensitive filename tie-break. The importer refuses an empty set, duplicate names, unsupported extensions, and a duration too short to give every panel a distinct frame at the project fps.
3. Each selected file is copied to a fresh subfolder under `sources/`, without overwriting an existing source. The ordered panels have equal frame spacing within the shot, starting at frame zero. Each panel holds one image layer fitted to the project format. Existing sources and shots remain unchanged.
4. The document change is one validated `commit_ops` batch and one undo step. A stale overlapping base conflicts rather than overwriting another edit. Undo removes the shot while leaving imported source files available in `sources/`.
5. The workbench shows upload and commit progress without blocking typing, selection or dragging. Errors say which file or stage failed. An agent's CLI import appears in the open workbench through normal live updates.

## Non-goals

Sequence video playback, editing imported images, deleting source files on undo, or importing an entire folder tree.
