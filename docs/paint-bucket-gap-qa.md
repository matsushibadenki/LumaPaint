# Paint bucket: gap closure and region offsets

Implemented 2026-10-07 as the first part of advanced line-art fill in the roadmap.

## Usage

Select Paint Bucket (Shift+G), click the active tool icon or double-click to open Tool Settings.

- Close gap: radius 0–16 in document pixels. Requires Contiguous. The default is 0.
- Grow / shrink: −32–32 in document pixels. Positive grows the region beneath outlines; negative contracts it. The default is 0.
- All layers can sample visible vector or pixel outlines while painting only the selected pixel layer. Individually designated reference layers are available in Stage 2.

Both controls are localized in English, Japanese and Simplified Chinese. Settings live in Rust; the WebView sends numbers only. Legacy requests without the new fields deserialize with zeros.

## Algorithm and limits

Gap closure erodes the seed-matching candidate area with a square kernel, floods its reachable component, then grows that component back and intersects it with the original matching pixels. This seals narrow passages without altering the reference image. A nearby surviving seed is used when erosion removes the clicked pixel; if no candidate remains within the radius, the operation changes nothing. Thin areas and narrow features may be omitted. This is a radius control, not a guarantee that every gap of a particular width will close.

Offsets use a separable sliding-window maximum/minimum on fill coverage. Each pass is linear in the pixel count, independent of radius, and runs on the existing Rust paint worker. Region analysis remains CPU work; the existing fill blend path continues to prefer GPU. No interactive speedup or maximum-resolution performance claim is made.

Anti-aliasing precedes the signed offset. Growth can extend under outline pixels by design. The final mask is clipped to the selection and destination alpha is preserved when alpha lock is enabled. Global (non-contiguous) fill ignores gap closure but supports offsets. Kernel windows are clipped at the canvas edge; pixels outside the canvas are never synthesized.

## Verified

- Renderer bucket/selection tests: 11 passed, including white and transparent broken outlines, disconnected global fill, positive/negative offsets, selection limits, alpha lock, no-op erosion, untouched references and all existing fill behavior.
- Sliding morphology equals a brute-force implementation on 1×1, 1×7, 7×1 and 8×5 images, for radii 0, 1, 3, 16 and 32, with both min and max coverage.
- Core settings test: legacy deserialization, round-trip and invalid-radius/offset rejection passed.
- macOS document bucket tests: 4 passed, including reference-outline filling on a separate layer, unchanged original outline, exact Undo/Redo document restoration and native save/reload pixel equality. Existing alpha-lock tests now exercise positive region offsets too.
- TypeScript and production Vite build passed. Workspace Clippy with warnings denied, formatting, diff whitespace and core dependency-boundary checks passed.
- UI flow: Tool Settings controls → change gap to 3 and offset to −2 → turn off Contiguous → gap control disabled while values retained. Verified in three languages, at desktop 960×700 and narrow 375×700, without horizontal overflow or application runtime errors.
- Browser plugin not available; bundled Playwright Chromium used on a local Vite server with the real React component and styles. The UI harness supplies an onChange callback and does not exercise native IPC. Native edit commands are covered by the Rust integration tests above. Screenshot saved outside the repository at `/private/tmp/lp-fill-settings.png`.

## Remaining verification and work

Full interactive WKWebView painting on production artwork, large-resolution latency/memory measurement, weak/diagonal/semitransparent outlines, sketch-layer attributes, vector-centerline filling, enclosure fills and leftover cleanup remain. The existing application bundle was not restarted, to preserve unsaved user work.

## Stage 2: selected reference layers (2026-10-07)

The Sample menu now offers Current layer, All layers and Selected references. Tool Settings lists visible pixel/vector/SVG layers with checkboxes (up to 64). Layers not checked, including sketch artwork, do not contribute to the sampled image; the selected destination remains unchanged. Hidden sources cannot be added, and an already selected hidden source can be unchecked. Deleted IDs are reported and can be cleared. An empty, stale or entirely hidden reference set causes filling to stop rather than falling back to all layers.

Exclude editable text removes native text objects from the read-only sampling scene. Rasterized lettering and unconverted SVG text remain as artwork; this option cannot identify those. Original sources, metadata and text are untouched. Source layer/group visibility, existing masks and opacity remain in the sampled scene. This stage does not introduce a sketch-layer attribute: exclude a sketch by leaving its checkbox off.

The initial Stage 2 implementation kept settings in a native-session map. Stage 3 below replaces that map with persisted Rust Document state and metadata history. Switching documents/pages cannot silently reuse matching layer IDs from another scene.

Verified: two core sampling tests cover flags, stale/duplicate/empty/hidden references, base references and non-destructive text exclusion. Five macOS paint-bucket integration tests pass, including document/page isolation and a reference-outline fill which ignores a full-canvas black sketch on another layer, preserves the original outline, restores exact Undo/Redo document state and reloads identical filled pixels. Renderer fill tests remain 11 passing; legacy settings default to current-layer sampling with no references or text exclusion. TypeScript/production build, workspace Clippy with warnings denied, formatting and architecture-boundary checks pass.

The real React controls were exercised through bundled Playwright Chromium (Browser plugin not available): reference mode, multiple sources, hidden-source disabling, text exclusion, mode changes retaining chosen IDs, three locales and narrow 375px layout; no application runtime errors or horizontal overflow. UI harness uses an onChange callback rather than native IPC; native editing is covered by Rust tests. Screenshot `/private/tmp/lp-fill-references.png` is outside the repository. No app restart was performed.


## Stage 3: document persistence and metadata history (2026-10-07)

All Paint Bucket settings, including source IDs, are now owned by Rust Document and saved per page in Native Format. Default settings are omitted from serialized metadata, so legacy documents remain compatible without a format-version change. Duplicating a page inherits its settings; adding a page starts with defaults. Loading rejects invalid ranges, duplicate IDs and nonexistent references before changing the document.

Settings edits record only settings metadata in Undo/Redo. They leave SVG sources and the scene journal unchanged. Deleting a reference layer clears its ID; undoing deletion restores both layer and reference. A new layer reusing a deleted ID does not become a reference. Plain pixel-layer deletion now records history as well. A reference mode with no sources remains enabled and safely stops filling until references are chosen again.

Verified: all 273 core tests pass, including five new persistence/history/page/deletion tests. Three native codec tests pass, including two-page byte-for-byte re-encoding and corrupt-reference rejection. Five macOS fill integration tests pass. Production build and workspace Clippy with warnings denied pass.

The real React controls hydrate saved settings, refresh after document-change/Undo notifications, and retain edited, unapplied dialog drafts. Bundled Playwright Chromium verified these flows with a mocked Tauri bridge, no runtime errors or horizontal overflow at 375px; screenshot `/private/tmp/lp-fill-persistence.png` was visually reviewed. Rust integration tests cover native processing; this harness does not establish full WKWebView interaction. The running application was not restarted.
