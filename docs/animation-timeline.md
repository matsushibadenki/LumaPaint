# Animation timeline

## Implemented scope

The bottom **Timeline** tab opens a resizable panel and starts closed. Standard macOS documents support pixel cels and vector keyframe tracks on the same frame ruler. Timeline editing and saved data live in Rust. The WebView sends commands and receives frame numbers, track labels and keyframe values; it receives no cel images or document rasters.

- Pixel layers: record the current drawing at a frame, create an empty cel, duplicate the held cel to the next frame, load a cel for painting, and delete an exposure. A cel holds until the next exposure or the end of the timeline. Before its first exposure the layer is empty. Empty cels can end a held drawing.
- Vector layers, including imported SVG artwork: independent X/Y position, X/Y scale, rotation and opacity keys. Values interpolate with Linear, Hold or Ease in/out. Position values are offsets in document pixels; scale and opacity are percentages. Rotation and scale use the canvas center as a fixed pivot established with the first key. Ordinary layer appearance, clipping and effects remain in the normal rendering path.
- Keys can be selected, edited numerically, moved by dragging, or removed. Moving onto an existing key is rejected rather than silently replacing it. Adding/updating a key at the current frame deliberately replaces its value.
- Playback, pause, first/previous/next/last frame, ruler scrubbing, loop, frame-rate and duration controls. UI frame labels start at 1; saved frame indices start at 0.
- Native `.lumapaint` save/reload and recovery include animation. Older files without animation data load an empty 24 fps / 120-frame timeline. Frame edits and keys participate in the normal Undo/Redo stack. Empty-cel creation clears the editable artwork in the same undoable operation.
- Japanese, English and Simplified Chinese labels. Narrow panels move the inspector below the tracks; both areas scroll independently.

## Pixel workflow

1. Select a pixel track. Track selection also selects that layer for painting.
2. Draw normally, then choose **Record drawing** at the desired frame.
3. Scrub to another frame and choose **Blank cel**, or duplicate an existing exposure with **Duplicate next**.
4. Use **Edit cel** to load a recorded cel into the painting tools. After editing, choose **Record drawing** to update the current exposure.
5. Play to preview. **Return to drawing** restores the editable artwork. Closing the timeline stops playback and exits preview.

Recording is explicit: painting does not automatically overwrite existing recorded cels. Preview blocks canvas drawing so it cannot accidentally change hidden source artwork. Selecting a different track exits preview and selects the editable layer.

## Vector workflow

1. Select a vector track and one of its transform or opacity properties.
2. At the first frame, enter the starting value and add a key.
3. Scrub to another frame, enter the ending value and add another key.
4. Choose an interpolation mode on the outgoing key. Hold makes an immediate change at the next key; Linear has constant value change; Ease in/out uses a smooth cubic transition with zero endpoint slope.
5. Drag a diamond to change its frame. Additional properties can use independent keys.

This follows the layer/property/key organization of [After Effects keyframe interpolation](https://helpx.adobe.com/ie/after-effects/desktop/animate-in-after-effects/animation-keyframes/keyframe-interpolation.html), and the exposure workflow of [Photoshop frame animation](https://helpx.adobe.com/uk/photoshop/desktop/add-video-and-animation/create-animation-frames/create-frame-based-animations.html). It does not claim full After Effects or Photoshop feature compatibility.

## Model and playback

`document::animation` stores settings and per-layer tracks. Pixel source/stroke data is shared through immutable `Arc<Cel>` references so duplicate exposures and timeline history do not repeatedly copy all artwork. Timeline edits are validated before commit. Shortening duration past authored data is rejected. A layer or page switch, changed document revision, document closure, or editor closure invalidates the Rust preview. Passive timeline queries do not finish an active paint gesture.

The Rust `Instant` clock computes the frame from elapsed time, avoiding accumulated timer drift. The UI requests ticks serially; slow rendering skips overdue frames. The native renderer retains a preview document and updates animated layers. Holds that evaluate to the same picture do not regenerate the document or request another GPU draw. Native painting tile previews are invalidated when switching to animation so they cannot display pixels from the editable drawing over a cel.

Limits: 1–120 fps, 1–18,000 frames, 256 tracks, 2,048 stored cels, 4,096 keys per property / 32,768 overall. Cel data has a 48 MiB aggregate budget, with the existing 64 MiB native project size limit still applying. Frame-rate selection is not a performance guarantee; complex effects and SVG content remain subject to the current renderer's cost.

## Current exclusions

Animation on large tiled raster documents, GIF/video/image-sequence export, audio, onion skinning, cel thumbnails, editable pivots, graph handles, motion paths, parented animation, and path/shape morphing are not implemented. Existing static export commands export the editable artwork, not a video or the transient animation preview. Animation support remains tied to the existing macOS native canvas; Windows/Linux native playback is pending.

## Verification

- Core tests: independent interpolation, clock/loop/end handling, held and blank cels, clone/load/Undo/Redo, settings validation, rejected edits preserving document/history/revision, legacy files and malformed cels.
- Renderer tests: exact pixel comparison for translated and partially transparent vector frames; pixel holds and blank frames; image equality after Undo/Redo and native save/reload.
- Native runtime tests: passive queries preserve live paint; document revisions stop stale preview/playback; closing a document releases its preview.
- Browser fixture: actual React component and styles with a mocked command transport; transport controls, cel recording/blank/duplicate/load, key updates/drag, closing during playback, error/focus states, three languages, light/dark themes and widths 320/375/414/768/1280. This verifies UI wiring and layout, not native GPU playback timing.

Validation on the development Mac (2026-10-06): `cargo test --workspace --offline` passed 722 tests (36 hardware/benchmark tests ignored); workspace Clippy with warnings denied, core dependency boundaries, TypeScript and the production frontend build passed. The macOS debug `.app` bundle was built successfully. GPU playback frame-rate and end-to-end native pointer interaction have not been benchmarked in this pass.
