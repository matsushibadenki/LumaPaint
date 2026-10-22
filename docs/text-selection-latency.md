# Text-box selection latency (2026-10-07)

A user reported roughly seven seconds between clicking a text box and seeing it selected. The exact user document has not been reproduced or timed in this pass.

## Identified synchronous path

The renderer compared an object-layer cache's selected IDs with the current selection even when the drag offset was zero. Selecting part of a layer invalidated the cache and called `prepare_object_cache` synchronously. That method splits selected/stationary runs, rasterizes SVG/text and creates/uploads GPU images. A text-heavy layer can therefore do substantial artwork preparation solely to display a selection outline. This path also ignored the deferred SVG worker for cold object images.

## Change

At zero offset, retained run images have the same viewport transform and compose to the same picture regardless of their selection tags. Reuse them when content, opacity and dimensions still match. Once dragging starts, selection IDs must match the retained partition; otherwise prepare the correct partition. Complete-layer selection retagging remains supported. Source edits, opacity and document-size changes still invalidate cached artwork.

In deferred native rendering, a missing/invalid object cache at zero offset falls through to the existing SVG cache/worker rather than synchronously preparing split images. Invalid object runs are excluded from composition so edited artwork cannot be covered by a stale object cache. No changes to text metrics, glyph positions, saved artwork or Undo were made.

## Verification and limits

Regression coverage checks stationary selection reuse and changed-selection rejection when dragging. A real multilingual two-text-object document preserves rendered pixels, SVG sources, document revision and scene journal generation while selecting each box, both boxes or clearing selection. Existing object-cache tests cover text changes, geometry, translation and Undo/Redo invalidation. The complete renderer suite passed 163 tests; 32 GPU/manual tests were excluded by their existing ignore attributes. Workspace Clippy with warnings denied, formatting, dependency boundaries and the macOS debug binary build also passed.

This is a targeted rendering-path fix, not a measured guarantee that the reported document now selects within a particular time. Full native click latency on that document remains to be checked. First drag preparation may still rasterize split runs synchronously; subsequent work should reuse finer-grained images or prepare them asynchronously. The running app is not restarted, preserving unsaved artwork.


## Drag-start follow-up

The user also reported approximately two seconds before motion starts. The first nonzero-offset render entered object-cache preparation before checking whether the existing complete layer texture could be translated. It also prepared object runs for stationary layers when their object cache had not been populated during deferred selection rendering.

Moved reuse checks ahead of preparation. Unchanged stationary layers keep their displayed texture. Entire selected layers, including a layer containing one text box, translate their retained fully-contained texture through the existing viewport uniforms without reshaping text, creating split images or uploading pixels. Source, opacity and document-size mismatches still reject reuse. Partial selections within one layer and artwork extending outside the retained image remain on the precise existing fallback; these can still require initial split-image preparation. This follow-up does not claim a measured elimination of all drag latency in every document.

Regression fixtures cover partial/whole/empty selections, exterior-content rejection, and exact pixel equality between translating retained Japanese/English/Chinese text and regenerating the translated text SVG. The actual document's end-to-end drag latency remains unmeasured.

Validation: the full renderer suite passed 165 tests (32 existing manual/GPU tests ignored). The retained object-texture translation test was then run explicitly on Apple M4 / Metal and passed, including movement across artboard bounds without reupload. The sandbox did not expose a Metal adapter; the same test succeeded outside that restriction. Workspace Clippy with warnings denied and the macOS debug binary build passed. No running-app restart or end-to-end timing was performed.


## Color-slider flicker follow-up (2026-10-22)

The native color command regenerated the changed vector layer, then requested a deferred frame without marking the old complete frame for retention. A cold/invalid layer cache is deliberately excluded from deferred composition. Consequently, while the SVG worker prepared the replacement, that frame could omit the entire edited layer, including its unselected objects. This is an identified code path; the user's exact document has not been reproduced.

After an actual fill/stroke paint change, the host now marks the current document/revision/canvas token for complete-frame retention using the existing vector-commit mechanism. Worker scheduling continues, but no incomplete replacement is presented while that generation is missing. A newer slider edit replaces the pending generation; readiness clears it. No-op edits add no wait. The paint-source/eraser rendering branch now respects this retention too, covering mixed vector/pixel documents. Rust retains artwork and render resources; no frame data is sent to the WebView.

Verification: 8 targeted native-host tests pass: repeated color-command edits preserve an unselected neighboring object and each pending generation; existing raster readiness, stale document/revision/canvas rejection, frame retention, import and export regressions also pass. Host Clippy (lib/tests, warnings denied), formatting, architecture boundaries and diff checks pass. These tests do not exercise a physical GPU surface. Physical native-window slider interaction, visual confirmation on the reported document and measured color-update latency remain pending. The running application was not restarted, preserving unsaved work.
