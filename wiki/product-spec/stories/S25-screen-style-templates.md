# S25: Reusable screen style templates

Status: owner requested, implementation pending

## Context

The NB Storyboard workbench is the destination for the scene graph, editable keyframe and composition workflow of an earlier prototype editor. That prototype is reference code only. Its editable keyframes already carry backgrounds and independent text, solid, image and annotation layers; its composer arranges interface screens and overlays. A sequence of related screens currently needs repetitive manual styling.

## Story

An editor creates a named style template from an editable screen, applies it to several storyboard screens, and can revise the shared style once without changing each screen's words, images, timing or media sources. Claude sees the template definition and assignments when asked to revise the sequence.

## First usable behavior

1. A project owns named templates. Each template records the visual properties it controls: background, typography, color, geometry and shared non-content decoration. Template identifiers remain stable across save/reopen and renaming.
2. An editor can create a template from a selected editable panel, assign it to a selected series of panels or shots, change the template, remove an assignment, and undo each action. The UI shows the active template and the panels it affects before a change is committed.
3. Applying or updating a template preserves each panel's text, image/video source, panel time, shot duration, cut placement and VO. Panels may override individual style properties; an override is visible and can be cleared.
4. Rendering, player preview and exported storyboard video resolve the same effective style. A template change invalidates affected preview and export caches.
5. The Claude brief and MCP `show` include template names, definitions, assignments and overrides. Agent edits use normal project operations and remain undoable.
6. Imported MP4 frames and other flattened images remain source pixels. A style template can govern editable overlays or a reconstructed screen, but must not claim to rewrite embedded raster text.

## Acceptance

- In a disposable project, create two panels with different text and image sources, assign one template, change its typography/color, and verify both renders change while text and source paths remain distinct.
- Save, close and reopen: template identity, assignments, overrides and rendered result persist. Undo template creation, assignment and edit separately.
- The same style appears at the corresponding player frames and in exported board-mode MP4. Verify a template edit causes fresh renders, including with a warm cache.
- A selected-series Claude brief and MCP `show` expose sufficient template data for an agent to follow it without reading UI state.
- The workbench offers an explicit path for editable screen content carried over from the prototype; a raster-only imported movie is labeled as such.

## Prototype migration boundary

Audit the separate editor's scene graph, keyframe/layer editor, composer, motion, audio and review controls against NB Storyboard before porting. Reuse behavior and source assets only where their dependencies and rights are available. Do not copy its browser-local persistence or disconnected JSON editing into the workbench's project/ops path.
