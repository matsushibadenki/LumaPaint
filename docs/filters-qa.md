# Categorized filters — 2026-10-26

The Filter menu now has Pixel, Vector and Animation submenus with capability badges and Japanese/English/Simplified Chinese labels. This first stage supplies working non-destructive presets through existing Rust-owned layer effects and keyframe state, not placeholders or a new bitmap transfer pipeline.

| Category | Initial operations | Target |
| --- | --- | --- |
| Pixel | Grayscale, Invert curve, Enhance contrast | Selected standard paint layer |
| Vector | Grayscale, Invert curve, Enhance contrast | Selected standard vector/SVG layer; original geometry/source is retained |
| Animation | Hold, Linear, Smooth | All property-channel keyframes on the selected unlocked vector/SVG layer |

Color presets affect the entire layer, not the pixel selection or selected vector objects. Invert replaces the composite curve with a descending linear curve; it is not a baked inversion after every later adjustment. Contrast sets the existing contrast field to +25. Grayscale sets saturation to −100, vibrance to zero, resets per-channel RGB curves and disables grading saturation so existing color settings do not reintroduce color. Exposure, contrast, composite tone, grading luminance, masks and screentone settings remain; existing processing order (including later screentone ink/paper) still applies. Numeric color controls remain available in the Layer Effects panel. Presets replace their owned fields and are idempotent; repeated application does not stack another independent filter node.

Animation presets change interpolation, not keyframe frame/value, cel exposure or motion-clip timing. They commit one animation history entry and preserve other tracks. Empty keyframe tracks and pixel-cel targets are rejected with a localized error. Playback/current-frame preview is refreshed through the existing history-preview path after a real state change. No new animation model or persistence schema is introduced.

The core validates category/preset compatibility, active strokes, content/channel context, lock state and layer existence before committing. The native command additionally checks document ID, revision, selected layer and native gesture state before editing; stale operations cannot target another active document/layer. The command runs through the existing main-thread session dispatcher and serialized JavaScript command queue. The UI blocks repeated actions while busy and disables categories incompatible with the selected layer. Requests carry scalar state only. Existing Rust/wgpu effect rendering performs the image work; the UI never receives a frame buffer for filtering.

## Verification

- Core library: 413 passed / 2 ignored. New filter cases check exact default inversion pixels, alpha preservation, grayscale with preexisting vibrance/RGB curve/grading, preserved tone settings, invalid category/preset/lock rejection, no-op history, atomic Undo/Redo and sampled animation interpolation/value/frame preservation.
- Native command regression: one test passed, rejecting stale document ID/revision and wrong layer without history changes, then applying a valid filter through the real native command function. This is a Rust session test, not a physical window/GPU acceptance run.
- UI typecheck and production build passed. The existing large-chunk advisory remains; no bundling policy changed.
- Core/native all-target Clippy with warnings denied, formatting, diff and core-boundary checks passed.
- Browser plugin/skill not available; regular Playwright used installed Chrome in headless mode because its own bundled Chromium executable was absent. No browser download was needed. The harness mounted the actual WorkspaceMenu with document fixtures and callback capture. Pixel/vector grayscale dispatch, animation Hold dispatch, correct category disabling, and lock/mask/busy guards passed in all three languages. Page runtime errors: zero. Screenshot: `/tmp/lumapaint-filters-menu.png` (1200×800).

The browser fixture validates rendered menus/dispatch, not native IPC, actual artwork pixels or playback. Actual Rust pixel math and interpolation are covered by core tests. Native GUI save/reload, animation playback/seek, effect rendering/export and undo interaction remain Pending, along with Windows/Linux, tiled documents, physical pen input and large-document latency. The earlier user request to defer real-app comparisons is respected; this task did not start native timed input or claim a new Release latency result.

## Next increments

- Parameter/preset management, explicit filter enable/reorder and selection-limited scopes, with preview/confirm/cancel ownership and one Undo transaction.
- Pixel Gaussian blur/sharpen using shared Rust/wgpu kernels and bounded worker previews; no full-image IPC.
- Vector path filters such as offset/simplify/roughen with original source preservation and selected-object semantics.
- Animation keyframe-range operations and pixel-cel exposure filters such as reverse/retime, with frame/value invariants, collision handling and multi-selection preservation.
