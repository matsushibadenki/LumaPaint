# Brush pressure response

## Implemented scope

🟢 [Done] Paint brushes and erasers use independent size and opacity response curves. Each curve has fixed input knots at 0%, 25%, 50%, 75%, and 100%; the three interior output values are editable. Outputs are monotone and stay in 0–100%. Minimum output is a floor, rather than a multiplier. Disabling a channel produces 100% output for that channel.

🟢 [Done] The default response preserves older strokes: linear size with a 5% minimum and pressure-independent opacity. Missing pressure settings in older files deserialize to this default. New strokes retain their complete response settings and raw pressure samples in native files and through Undo/Redo. Changing the current brush does not rewrite existing artwork.

🟢 [Done] The existing eleven brush presets load their associated pressure response. The pressure panel can load, save, or reset the response for each brush. Saved overrides live in `brush-pressure-v1.json` in the application configuration directory; validated updates use a temporary file and replacement. The curve selector provides linear, light-pressure, firm-pressure, and S-curve templates. Templates and minimum output can be edited independently for size and opacity.

🟢 [Done] English, Japanese, and Simplified Chinese labels, tooltips, keyboard-editable curve points, and collapsible blocks are available. Up/Down changes a curve point by 1%; Shift changes it by 10%; Home/End moves it to the adjacent output bound. Presets and curve edits change subsequent painting settings.

## Rendering and scratch pad

- Pressure normalization and response evaluation live in `lumapaint-core::pressure`. Invalid stored curves are rejected before drawing.
- Rust's shared arc-length sampler evaluates pressure after interpolating input samples. It creates matching CPU raster dabs and GPU segments. Pressure opacity uses the existing calibrated optical-depth/flow deposition, together with the brush's opacity, alpha, hardness, envelope, and material. Repeated passes can accumulate paint.
- Portable dab radius and optical-depth limits also apply near zero output. A zero-output size or opacity sample deposits no paint; it does not generate a non-finite dab.
- The scratch pad is a separate Rust-owned state per editor window, limited to eight strokes and 256 samples per stroke. It never edits a document, its revision, or its Undo stack. The same raw scratch traces are re-rendered when brush settings change.
- Pen events use their supplied pressure; mouse input uses the explicit mouse-pressure slider. The sample button generates a 0–100% pressure ramp in Rust. Pointer cancellation removes the unfinished scratch stroke. A new view token prevents late input from an old panel from cancelling a new gesture.
- Small 256×128 previews use the existing Rust tiled brush renderer and return opaque `pressure-preview` URLs. Pixel arrays do not cross the command bridge. This cache is separate from document previews, bounded to four recent images per window and 64 total; closing a window releases its scratch state. The main canvas continues to use Rust/wgpu.
- Retouch tools, vector stroke styles, and tablet-driver calibration have separate settings. The scratch pad is a response comparison surface, not a document export editor.

## Validation

🟢 [Done] Initial core tests cover pressure normalization, legacy defaults, interpolation, independent minima, valid brush defaults, rejection of malformed curves, native round trips, and Undo/Redo. Renderer tests compare shared CPU/GPU dab parameters and stationary opacity outputs at 0%, 25%, 50%, and 100%. Scratch tests cover bounds, cancellation, and re-rendering through the actual brush renderer.

🟢 [Done] The real React pressure panel was exercised with a test transport in English, Japanese, and Simplified Chinese at 260px and 308px: independent curve changes, minima, keyboard editing, per-brush save/load/reset, existing preset integration, mouse-pressure command payloads, scratch begin/append/end, clear, and sample. No overflow or runtime errors were observed. Browser skill was unavailable; Playwright was used.

🟢 [Done] Final core suite: 463 passed (4 ignored); renderer suite: 318 passed (79 ignored); native pressure tests: 3 passed. Workspace all-target Clippy with warnings denied, formatting, frontend build, architecture boundaries, and notification regression checks passed. The explicit custom-response GPU comparison passed on Apple M4 / Metal: maximum channel difference 1, mean difference 0.026 across 2,218 painted pixels.

🟢 [Done] Built and exercised a separate macOS Tauri QA bundle with isolated preferences, preserving the user's open document. Verified independent firm size / light opacity curves, a 20% size minimum, scratch image loading through the custom protocol, mouse traces and evaluated output (18% input → 3.2px at 16px diameter / 36% opacity; 50% → 4.8px / 75%), clear/sample, existing graphite and round preset integration, and saved round response restoration after application restart. Scratch operations kept the document saved. An actual canvas stroke was saved and its native JSON retained both custom response curves and minima.

⭕️ [Pending] Native verification in English and Simplified Chinese; their React UI interactions and layout were tested in the browser.

⭕️ [Pending] Physical pen tablets, device-specific pressure/tilt/driver behavior, and native Windows/Linux interaction. Synthetic input and mouse testing do not replace hardware calibration.
