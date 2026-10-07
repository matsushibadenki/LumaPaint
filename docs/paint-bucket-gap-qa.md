# Paint bucket: gap closure and region offsets

Implemented 2026-10-07 as the first part of advanced line-art fill in the roadmap.

## Usage

Select Paint Bucket (Shift+G), click the active tool icon or double-click to open Tool Settings.

- Close gap: radius 0–16 in document pixels. Requires Contiguous. The default is 0.
- Grow / shrink: −32–32 in document pixels. Positive grows the region beneath outlines; negative contracts it. The default is 0.
- All layers can sample visible vector or pixel outlines while painting only the selected pixel layer. Individually designated reference layers are still planned.

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

Full interactive WKWebView painting on production artwork, large-resolution latency/memory measurement, weak/diagonal/semitransparent outlines, dedicated reference layers, vector-centerline filling, enclosure fills and leftover cleanup remain. The existing application bundle was not restarted, to preserve unsaved user work.
