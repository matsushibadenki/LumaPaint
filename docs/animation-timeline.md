# Animation timeline

## Implemented scope

The bottom **Timeline** tab opens a resizable panel and starts closed. Standard macOS documents support pixel cels and vector keyframe tracks on the same frame ruler. Timeline editing and saved data live in Rust. The WebView sends commands and receives frame numbers, track labels and keyframe values; it receives bounded thumbnail URLs, never full cel images or document rasters.

- Pixel layers: record the current drawing at a frame, create an empty cel, duplicate the held cel to the next frame, load a cel for painting, and delete an exposure. Drawings have independent IDs and names in a per-track cel library. Exposures reference those IDs and define half-open display intervals, with empty gaps allowed. Legacy holds migrate to the same intervals. Removing an exposure keeps its drawing available for reassignment.
- Vector layers, including imported SVG artwork: independent X/Y position, X/Y scale, rotation and opacity keys. Values interpolate with Linear, Hold or Ease in/out. Position values are offsets in document pixels; scale and opacity are percentages. Rotation and scale use the canvas center as a fixed pivot established with the first key. Ordinary layer appearance, clipping and effects remain in the normal rendering path.
- Keys can be selected, edited numerically, moved by dragging, or removed. Moving onto an existing key is rejected rather than silently replacing it. Existing-key value edits update the artwork automatically after a short (120 ms) input debounce; Enter or leaving the field applies pending input immediately. Interpolation edits also update the existing key. Empty/non-finite/out-of-range input is not saved. At frames without a key, values remain drafts until Add/Update is pressed. Adding/updating a key deliberately replaces its value and previews that key’s frame. Key edits carry a document ID to reject delayed updates after a document switch.
- Playback, pause, first/previous/next/last frame, ruler scrubbing, loop, frame-rate and duration controls. UI frame labels start at 1; saved frame indices start at 0.
- Native `.lumapaint` save/reload and recovery include animation. Older files without animation data load an empty 24 fps / 120-frame timeline. Frame edits and keys participate in the normal Undo/Redo stack. Empty-cel creation clears the editable artwork in the same undoable operation.
- Japanese, English and Simplified Chinese labels. Narrow panels move the inspector below the tracks; both areas scroll independently.

## Pixel workflow

1. Select a pixel track. Track selection also selects that layer for painting.
2. Draw normally, then choose **Record drawing** at the desired frame.
3. Scrub to another frame and choose **Blank cel**, or duplicate an existing exposure with **Duplicate next**.
4. Use **Edit cel** to load a recorded cel into the painting tools. After editing, choose **Record drawing** to update the current exposure.
5. Play to preview. **Return to drawing** restores the editable artwork. Closing the timeline stops playback and exits preview.

Pointer-down with a painting tool loads the held cel for editing at the current frame. Seeking, playing or returning to drawing records pending edits at that frame. Record drawing can also commit explicitly. Hand, zoom and eyedropper preserve preview.

## Vector workflow

1. Select a vector track and one of its transform or opacity properties.
2. At the first frame, enter the starting value and add a key.
3. Scrub to another frame, enter the ending value and add another key.
4. Choose an interpolation mode on the outgoing key. Hold makes an immediate change at the next key; Linear has constant value change; Ease in/out uses a smooth cubic transition with zero endpoint slope.
5. Drag a diamond to change its frame. Additional properties can use independent keys.

This follows the layer/property/key organization of [After Effects keyframe interpolation](https://helpx.adobe.com/ie/after-effects/desktop/animate-in-after-effects/animation-keyframes/keyframe-interpolation.html), and the exposure workflow of [Photoshop frame animation](https://helpx.adobe.com/uk/photoshop/desktop/add-video-and-animation/create-animation-frames/create-frame-based-animations.html). It does not claim full After Effects or Photoshop feature compatibility.

## Model and playback

`document::animation` stores settings and per-layer tracks. Pixel source/stroke data is shared through immutable `Arc<Cel>` references so duplicate exposures and timeline history do not repeatedly copy all artwork. Timeline edits are validated before commit. Shortening duration past authored data is rejected. A layer or page switch, changed document revision, document closure, or editor closure invalidates the Rust preview. Passive timeline queries do not finish an active paint gesture.

The Rust `Instant` clock computes the frame from elapsed time, avoiding accumulated timer drift. The UI requests ticks serially; slow rendering skips overdue frames. The native renderer retains a current-page preview document, omitting editing history and inactive pages, and updates animated layers. Holds that evaluate to the same picture do not regenerate the document or request another GPU draw. Native painting tile previews are invalidated when switching to animation so they cannot display pixels from the editable drawing over a cel.

Limits: 1–120 fps, 1–18,000 frames, 256 tracks, 2,048 unique stored cels and 2,048 exposures overall, 4,096 keys per property / 32,768 overall. Cel data has a 48 MiB aggregate budget, with the existing 64 MiB native project size limit still applying. Frame-rate selection is not a performance guarantee; complex effects and SVG content remain subject to the current renderer's cost.

## Current exclusions

Animation on large tiled raster documents, GIF/video/image-sequence export, audio, editable pivots, graph handles, motion paths, parented animation, and path/shape morphing are not implemented. Existing static export commands export the editable artwork, not a video or the transient animation preview. Animation support remains tied to the existing macOS native canvas; Windows/Linux native playback is pending.

## Verification

- Core tests: independent interpolation, clock/loop/end handling, held and blank cels, clone/load/Undo/Redo, settings validation, rejected edits preserving document/history/revision, legacy files and malformed cels.
- Renderer tests: exact pixel comparison for translated and partially transparent vector frames; pixel holds and blank frames; image equality after Undo/Redo and native save/reload.
- Native runtime tests: passive queries preserve live paint; document revisions stop stale preview/playback; closing a document releases its preview.
- Browser fixture: actual React component and styles with a mocked command transport; transport controls, cel recording/blank/duplicate/load, key updates/drag, closing during playback, error/focus states, three languages, light/dark themes and widths 320/375/414/768/1280. This verifies UI wiring and layout, not native GPU playback timing.

Validation on the development Mac (2026-10-06): `cargo test --workspace --offline` passed 722 tests (36 hardware/benchmark tests ignored); workspace Clippy with warnings denied, core dependency boundaries, TypeScript and the production frontend build passed. The macOS debug `.app` bundle was built successfully. GPU playback frame-rate and end-to-end native pointer interaction have not been benchmarked in this pass.

Document pointer-down with an editing tool now leaves animation preview automatically. For a pixel track with an exposure at the current frame, the held cel is loaded for drawing at the current frame; subsequent changes update that exposure’s referenced cel when seeking, playing or returning to drawing. All exposures of that cel share the update; Make cel unique isolates one exposure first. Passive timeline queries preserve this pending edit. Hand, zoom and eyedropper keep the preview intact. Frame changes invalidate the GPU tile ownership so cached textures from another exposure cannot suppress the current frame's artwork. Regression coverage includes held artwork, blank exposures, automatic loading, passive queries and recording edits before seeking away.

## Incremental evaluation and viewport rendering (2026-10-13)

Unchanged tracks now retain their evaluated layers, held cels and SVG sources. Native preview reuse is scoped to the same document and revision, and is reset after editing or preview recreation. Transport responses and notification refreshes omit unchanged timeline metadata using an explicit `dataUnchanged` flag. Both panels retain metadata only when that flag permits it; a closed document clears it. Timeline marks and keys are generated only for the visible horizontal range, including overscan and captured drag targets. The prior full-evaluation API remains available for validation. See [reference design, benchmark conditions and remaining scope](timeline-reference-2026-10-13.md).


## Native key editing validation (2026-10-13)

Tested the actual Tauri custom-protocol app on macOS / Apple M4 Metal at a 1200 × 800 window, with a 512 × 512 imported SVG rectangle. No mock transport was used for these checks. Changing frame 25's X key from 120 to 160 updated the canvas; frame 13 then evaluated to 80 instead of 60. Direct numeric seeking, key selection, forward steps, playback/pause, returning to drawing, saving and reopening were exercised. A composed case with X=160, Y=40, Scale X=150%, Scale Y=50%, Rotation=90°, and Opacity=50% retained all keys after reopening. Updating the existing opacity key to 100% while paused produced the opaque rotated rectangle without starting playback. At 50% display zoom the rotated rectangle occupied approximately 15 × 60 screen pixels, matching the 80 × 60 source rectangle and the authored scales; moving from frame 25 to frame 13 shifted it left by 40 screen pixels.

The frame input now retains its draft during asynchronous seek replies, preventing multi-digit entry from being replaced mid-input. Timeline presentation uses the native coalesced display update, and a redraw request always wakes the retained display link even when a pending flag remains after occlusion. These observations validate this macOS fixture and workflow; they do not certify every motion preset, operating system, frame rate, or large-document performance.


## Cel previews and onion skin — 2026-10-13

Pixel tracks show transparent-background cel thumbnails inside held exposures. Choose thumbnail size in the toolbar; clicking a thumbnail seeks to its exposure. Thumbnails reflect recorded cels, including edits committed by frame navigation. Enable **Onion skin** and adjust the selected pixel track in the inspector:

- Up to four previous and four following references, independently adjustable. **Cels** skips held repetitions; **Frames** uses a 1–24 frame interval. Blank/current cels and duplicate shared drawings are omitted, without wrapping at timeline edges.
- Original, Half color or Tint, separate previous/following colors, opacity and distance fade. Pin up to four absolute frames alongside relative references. A pinned duplicate keeps the stronger opacity.
- References remain visible while painting and disappear during playback. They are a display overlay, never part of the recorded drawing or static export. Preferences save with the document and support Undo/Redo. Opening the timeline evaluates the selected cel at its current frame.

This independently implements the exposure thumbnails and onion controls described by the [CLIP STUDIO Timeline Palette](https://help.clip-studio.com/en-us/manual_en/600_animation/Timeline_Palette.htm) and [Onion skin](https://help.clip-studio.com/en-us/manual_en/600_animation/Onion_skin.htm) documentation. Relative and fixed references follow the concepts in the [OpenToonz manual](https://opentoonz.readthedocs.io/en/latest/drawing_animation_levels.html#using-onion-skin) and its [OnionSkinMask interface](https://github.com/opentoonz/opentoonz/blob/master/toonz/sources/include/toonz/onionskinmask.h). No external source code or dependencies were copied.

Rust rasterizes 96 px thumbnails and references bounded to 1024 px on a worker. A shared cache is capped at 192 entries / 48 MiB and holds weak cel identities; it does not keep discarded source artwork alive. Thumbnail PNGs use an opaque native protocol token. One latest pending job per editor replaces obsolete scrub jobs, with document/generation validation before installing results. Holds with unchanged references reuse the overlay. The GPU receives one merged reference texture, at most 4 MiB; large raster frames do not cross IPC.

Actual macOS app verification used a disposable five-cel project: held thumbnails, two previous/two following tinted references, painting with references visible, edited thumbnail/neighbor updates after seeking, fixed references and playback hiding were confirmed. Settings and drawing were saved and reloaded in the final app bundle, confirming restored counts, colors, fixed references and painted strokes. Core, renderer, native runtime and cache tests cover validation, serialization/Undo, blank/held/reused cels, live paint preservation, stale jobs and playback. A real Apple M4 Metal test checks both RGBA/BGRA surface formats and half-alpha color correctness. The real React inspector was checked in Japanese, English and Simplified Chinese at 375 px without horizontal overflow; its temporary harness was removed.

The current scope is the selected pixel layer. Whole-scene onion skin, cross-layer cel levels, Shift & Trace transforms, and adaptive reference resolution above 1024 px remain future work. Windows/Linux and large tiled-document animation retain the existing native-canvas limitations.


## Independent cels and exposure editing — 2026-10-13

The pixel inspector has a **Cels** library with stable drawing IDs, editable names and use counts. Add creates an unassigned blank cel. Assign places the selected cel at the current frame or replaces that exposure’s reference; placing inside an existing hold splits its timing. Delete exposure removes only its timing. Delete cel is disabled while any exposure references it. Duplicate next reuses the same drawing identity. Make cel unique gives the current exposure its own ID, sharing immutable pixels until the drawing is edited.

Painting a loaded exposure updates its referenced cel on Record, seeking, playing or returning to drawing. This updates all placements of the same cel. Explicit recording at an unassigned start creates a new cel; recording at an existing exposure start updates its referenced cel. To paint a new unassigned library cel, assign it to a frame first and use Edit cel or draw on its preview.

Drag either edge to shorten/extend a hold, or drag its body to move the interval without changing its length. The live range display is local UI state; releasing commits one validated Rust edit and one Undo entry. Adjacent exposures bound the drag, with no implicit overwrite or ripple. Arrow keys adjust one frame, Shift+arrow adjusts ten; Escape/pointer cancellation abandons the drag. Numeric Start/End controls offer exact placement (UI end is the last included 1-based frame). Gaps render empty, including playback and onion references. Timing edits do not copy source pixels.

New saves store each named drawing once and lightweight exposure references separately. Old frame/cel arrays are accepted and migrated; ambiguous mixed data, duplicate IDs, missing references, overlapping/empty/out-of-range intervals, invalid names and budgets are rejected. A resolved `Arc<Cel>` map remains a Rust-only playback cache so existing renderer paths can reuse shared drawings. History accounting includes unassigned library drawings. New saves require this updated app; backwards writing for older app versions is not provided.

The initial scope is one library per pixel layer. Cross-layer level sharing, cross-layer multiselection and independent editing of unassigned cels remain later extensions.


Validation for exposure editing: 352 core tests (2 benchmark tests ignored), 9 native timeline tests, 9 renderer animation tests and the timeline UI helper checks passed. Actual Metal testing covers reference color/alpha for both 8-bit sRGB surface formats and RGBA16Float composition. Clippy with warnings denied, TypeScript/frontend build and dependency-boundary checks passed. The Japanese/English/Simplified Chinese library and edge controls were operated at 375 px with no inspector overflow or browser console errors; the temporary harness was removed.

The actual macOS app loaded the previous frame/cel format, shortened a hold, moved its body, resized its start, and restored timing with Undo/Redo. A cel was assigned to a second exposure and brush edits appeared in both; after Make cel unique, a second stroke remained exclusive to the fork. Exposure deletion left the named drawing available, and only the unused cel delete action was enabled. Ordinary RGB reference composition now sits beneath current artwork so an overlapping fork cannot tint the current drawing. Single-channel, grayscale and masked-paper views retain their existing reference-overlay path.

The final macOS bundle reloaded both assigned and unassigned named cels and their edited exposure intervals. The unassigned fork retained both brush strokes and was assigned again after reload. Playback and the corrected RGB overlap ordering were also verified in the actual app.


## Multiple exposure editing, ripple and reorder — 2026-10-13

Within one pixel track, ⌘/Ctrl-click toggles exposures and Shift-click selects the range from the anchor. Select all/Clear selection are in the batch toolbar; ⌘/Ctrl+A and Delete work when focus is outside form fields. Drag a selected body to move the group, with live timing previews for every affected visible exposure. Arrow keys move by one frame, Shift+arrow by ten. Edge edits apply to that exposure; the batch Hold length control sets all selected lengths. Successful batch edits map selected exposure occurrences to their new frame addresses using the Rust timing plan. Single interval edits select the edited occurrence. Unchanged frame/cel placements retain selection through other metadata revisions; unmatched occurrences are removed.

- Ordinary moves preserve offsets and gaps and reject final overlaps. Dragging is bounded by adjacent unselected exposures; numerical offsets may jump to another free interval. Ordinary hold changes preserve starts and reject overlaps. Ordinary deletion leaves gaps and keeps library drawings.
- Ripple move shifts **every exposure from the first selected start onward**, including unselected later exposures. Ripple hold changes selected durations and shifts subsequent exposures by the accumulated change. Positive overflow extends the timeline up to 18,000 frames in the same Undo operation. Negative changes keep the document duration, avoiding accidental truncation of other tracks.
- Ripple delete closes only the removed occupied intervals; existing gaps remain and trailing timeline space is retained. All referenced drawing identities remain unchanged.
- Reorder inserts selected exposures as a stable block before an unselected exposure, or at the end. Each cel retains its hold length; the original first start and the gaps between timing slots remain. It affects only this track and keeps the total occupied span. Ripple mode does not change reorder semantics.

Rust plans and validates the complete result before committing. One batch creates one timing history entry and no drawing copies. Native requests carry document ID and revision; stale revisions are rejected before recording pending paint. No-op edits create no history entry. UI move bounds use sorted interval searches, and selection/name lookups use sets/maps. Timing selection is view state; document data, playback and rendered artwork remain in Rust.

Core tests cover noncontiguous selection, overlaps/bounds/duplicate selection rejection, shared drawing pointers, ripple duration changes, deletion, gap-preserving reorder, no-ops, save/reload and Undo/Redo. Native tests cover stale revisions and a batch’s history/preview. The three-language batch toolbar was operated at 375 px with no overflow or browser console errors; the temporary harness was removed. Current extensions still pending are cross-track selection, drag-and-drop reorder between occupied slots.

Actual macOS app verification used a disposable six-cel project. Shift range selection, Select all/Clear, batch moves and holds, ripple duration extension, gap-preserving reorder and ripple deletion were operated through the native UI. Undo restored each whole batch, including timeline duration; drawings remained in the library. The reordered document was saved, closed and reloaded with the same intervals and cel names, then played and paused successfully. The final custom-protocol app bundle and frontend build passed. Core validation passed 355 tests (2 ignored benchmarks), and native timeline validation passed 10 tests.

Actual pointer dragging remains pending: the UI automation tool could not capture the app window for drag input. Move-bound and selection calculations are tested, and the shared Rust batch path is verified through native controls, but these checks do not substitute for a real pointer drag. Windows/Linux and large tiled-document verification also remain pending.

## Exposure edge scrolling — 2026-10-13

Dragging a body, a selected group, or either hold edge scrolls horizontally when the pointer is within 40 px of the track viewport edge. Speed ramps up to 720 px/second, using animation timestamps with a 32 ms maximum step. Narrow viewports reduce the edge band to one third of their width. Scroll displacement joins pointer displacement before conversion to frames; the existing neighboring-exposure and document bounds still apply. This does not grow the timeline during a drag beyond its existing scroll extent; ripple duration extension remains validated on commit.

The captured exposure remains mounted outside the virtualized horizontal range. Release, Escape, pointer cancellation, capture loss and unmount stop the animation loop. Intermediate previews remain local UI state, and release commits at most one existing Rust timing operation. Instructions are available in the Japanese, English and Simplified Chinese information tooltip.

Helper tests cover central/edge/outside positions, narrow and empty viewports, bounded speeds and combined pointer/scroll displacement. The actual React component was dragged in the browser: body movement scrolled 13 px and committed once, reversing the start edge scrolled back and committed once, and extending the end edge scrolled 15 px and committed once. Type checking and the production frontend build passed. The temporary harness was removed. Native-window pointer verification remains pending due to the previously observed automation window-capture limitation; browser results are not native-wgpu verification.

## Selection retention and cel library thumbnails — 2026-10-13

After moving, changing holds or reordering, the native response returns the new starts of the selected exposure occurrences. This uses the original occurrence’s start carried through the Rust plan, so two placements of the same drawing remain distinct. Ripple-following unselected exposures do not become selected. Deleting the selected occurrences returns an empty selection; a rejected edit retains the original selection. The UI stores the response with document ID and revision until that metadata is applied, avoiding a subsequent refresh discarding the mapping. Selection remains view state and does not add Undo entries or drawing copies.

The later persistent-ID extension below supersedes frame/cel matching: other revisions, including external Undo/Redo, now follow the selected occurrence IDs. Document/layer changes and explicit selection changes clear the previous identity selection.

The cel library shows named thumbnails and use counts for assigned and unassigned drawings, with 24 entries per page. Image buttons select a cel for the existing rename/assign/delete controls; selecting through the menu jumps to its page. The native thumbnail request’s optional `library` flag interprets addresses as cel IDs instead of exposure starts, and old requests retain their default behavior. The existing Rust raster worker, 96 px images, bounded cache and opaque native image protocol are reused. No source pixel buffers cross IPC. Revision/page/document changes invalidate UI results; late replies from abandoned pages are ignored. Empty drawings use a blank placeholder. Images can be stale until pending painting is recorded, as before.

Four core batch tests cover exact occurrence mapping, including reused cels, ripple, reorder, delete and invalid selections. Eleven native timeline tests cover the returned selection, unassigned library drawing identity, missing IDs, stale document IDs and the request budget. The actual React timeline with a fixture transport retained two shared-cel selections across two consecutive moves; library pagination and unassigned thumbnail selection worked. All displayed fixture thumbnails loaded. Japanese/English/Simplified Chinese library controls fit a 375 px container; after correcting duplicate sibling panel keys, language switching produced no console warnings/errors. This fixture verifies the UI; native source-resolution tests separately verify the Rust path. Type checking, the production frontend build, formatting and workspace-wide Clippy with warnings denied passed. Live app-bundle interaction and Windows/Linux remain pending.

## Persistent exposure identities — 2026-10-13

Each exposure now stores a bounded, opaque string ID independently of its frame and drawing reference. Batch edits, interval edits and assigning another cel at an existing exposure start preserve this ID. Splitting a hold creates a fresh ID for the inserted occurrence while preserving the original. New IDs use a process timestamp epoch and atomic sequence outside undoable document state, so a new edit on an Undo branch does not reuse an occurrence’s ID. These are track-scoped labels, not authentication tokens; no dependency was added.

Legacy exposures without IDs receive distinct IDs during load, and subsequent saves preserve them. Nonempty duplicate IDs and malformed IDs are rejected. Existing frame/cel migration remains supported. New saves require the updated reader because older builds reject unknown exposure fields.

The panel retains selected IDs and the anchor ID while resolving their current frame addresses after each authored revision. Deleted occurrences are temporarily absent from visible selection; Undo restores their selection when the same IDs return. Explicit new selection, Escape, document/layer changes clear the old identity set. A new occurrence using the same cel and frame cannot inherit selection from a deleted one. Selection itself remains unsaved view state, without history entries. The captured clip’s React key uses its ID, and timing/content changes cancel an active pointer drag to prevent a stale commit.

Core validation passed 358 tests (2 benchmark tests ignored), including ID persistence across reorder/Undo/Redo/save, legacy ID migration, duplicate rejection, hold splitting and new IDs on Undo branches. Eleven native timeline tests passed. UI helper tests cover anchor mapping, temporary deletion, restoration and same-frame replacement. The actual React timeline, using fixture metadata responses, retained two same-cel occurrences through movement, Undo, Redo, deletion and deletion Undo without console errors. This is UI response testing, not an actual app-bundle Undo operation. Frontend build, formatting, architecture boundaries and workspace-wide Clippy with warnings denied passed. Native app-bundle and Windows/Linux interaction verification remain pending.

## Native app verification — 2026-10-14

This verification supersedes the earlier macOS app-interaction and pointer-capture pending notes. The current source was built into the Debug custom-protocol app bundle (207.82 MiB) and launched afresh. A disposable 512 × 512 document contained three placements of one drawing, five unassigned drawings with distinct blue-dot positions, 24 frames and 6 fps. The temporary fixture generator was removed; existing user documents were not edited.

Actual UI operations confirmed two-occurrence range selection, group shift, Undo, Redo, selection restoration after deleting the group and Undo, and assigning an unassigned cel to an existing selected occurrence followed by Undo. The library showed real native-protocol thumbnails and selected an unassigned drawing from its image. Saving, closing, reopening and saving again preserved starts 1/4/12 (zero-based), all three exposure IDs and six library drawings. Reopening clears view selection as intended. Playback advanced from frame 4 to frame 6 before pausing.

At 32 px/frame, actual pointer dragging moved the group to starts 6/9 while preserving two selections and respecting the next exposure at frame 12. A 1.5-second hold at the right viewport edge scrolled the ruler to the later frames. This exposed a bug: when interval bounds prevented further movement, pointer release also invoked ordinary click selection, reducing the group to one occurrence. The fix classifies pointer displacement, scroll displacement, or an actual timing change as a drag, suppressing that click even when the final timing is unchanged. The next pointer-down clears any leftover suppression. Tests cover a bounded pointer move, a scroll-only gesture and a one-frame edit at narrow zoom.

The final rebuilt app repeated the bounded group move and edge hold: selection stayed at two before and after scrolling, and one Undo restored the prior timing. Scroll-only preview creates no extra history entry. The final app was left paused on the disposable document, with its saved intervals restored and two occurrences selected. Type checks, UI helper tests, frontend build, app compilation and bundling passed; no Rust processing or drawing data transfer behavior changed. Windows/Linux, left-edge/long-duration/multiple-monitor interaction and large tiled-document checks remain pending or future work.

## Collapsible layer details — 2026-10-14

Each track has a disclosure button independent of layer selection, plus Collapse all/Expand all. A closed layer uses a 30 px header with its name, type icon, lock indication and item count (exposures for pixel tracks, keys plus motion clips for vector tracks). Property/motion rows and pixel thumbnails are hidden; pixel exposure intervals remain as compact interactive bars. Selecting another layer does not automatically reopen it.

The selected layer’s inspector disclosure shares the same state. Closing it hides the details and reduces the desktop inspector width from 270 to 180 px; narrow layouts retain only a short inspector header. Inspector contents stay mounted while hidden, preserving library selection and drafts. Pending vector key edits are flushed before closing. Track thumbnail requests skip closed rows. Active drag guards prevent closing away a captured key/clip. Open/closed state is view-only, remains through metadata revisions, and resets when switching documents or recreating the panel; it is not saved in the project.

Browser checks confirmed individual/all folding, six property rows and one motion row returning on expansion, and Japanese/English/Simplified Chinese actions. At a 375 px fixture width the panel measured 343 px with no horizontal overflow; closed rows measured 30 px. A temporary-fixture hot reload produced a createRoot warning, separate from product behavior; the fixture was removed. The final custom-protocol macOS app opened a disposable two-layer document, collapsed and restored the vector X value 10 with its keys, and closed pixel/vector layers together. Type checking, frontend/app build and whitespace validation passed. Native Windows/Linux interaction and saving folding preferences remain future work.

## Drag insertion reordering — 2026-10-15

The exposure selection toolbar adds the ⇄ toggle (Drag to reorder) in English, Japanese and Simplified Chinese. When enabled, a body drag crosses occupied intervals instead of being clamped to empty space. The insertion line points before the remaining exposure whose midpoint has not yet been crossed, or after the last exposure. Selected exposures are excluded from targets; a multiple selection moves as an ordered block in the same track. If all exposures are selected there is no reorder target. The ghost is a view preview; final slot timing and gap preservation are computed by Rust.

Reordering and ripple movement are mutually exclusive. Hold edges and arrow-key timing adjustments keep their existing behavior. Edge scrolling, pointer capture, Escape/pointer cancellation, playback/locked-layer guards and persistent occurrence selection are shared with the existing drag path. The gesture sends one existing revision-checked exposureBatch/Reorder command on release, preserving cel references and hold lengths with one Undo; there is no drawing-data transfer during dragging. Document revision or mode changes cancel an in-progress drag so stale previews cannot commit. Mode and insertion preview are panel view state, not saved artwork.

Helper checks cover selected-target exclusion, midpoint boundaries, left/end insertion, all-selected/no-selection and invalid coordinates. The six existing Rust exposure-batch regression tests pass. The Debug/custom protocol macOS app verified A/B/C/D → B/C/A/D by a body drag, A+C → B/A/C/D, then append → B/D/A/C. Selection stayed at two through multi-edit and Undo/Redo. Save JSON comparisons verified every occurrence ID, effective hold length (including implicit end intervals), the complete cel library and 24-frame duration. Closing and reopening restored B/D/A/C with holds 4/12/4/4. The final rebuilt app, including the revision cancellation guard, repeated B/D/A/C → D/B/A/C and restored the saved order with one Undo. Controls rendered in all three languages at a measured 287 px inner width without overflow in the measured Japanese/Chinese cases; the browser console reported no errors. Type checking, frontend/app build, boundary checks and whitespace validation passed.

Windows/Linux native input and large-track interactions remain pending; editing selections across tracks and direct drawing of unexposed library cels remain next work.
