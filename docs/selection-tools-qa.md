# Selection tools QA — 2026-10-06

[Done] Paint workspace menu groups Selection Brush, Lasso, Polygonal Lasso and Magnetic Lasso. L selects Lasso; Shift+L cycles the four tools. Brush/freehand tools drag and commit on release. Polygonal/magnetic tools close with the starting point, double click or Enter; Escape cancels and Delete removes the previous anchor. Shift adds and Option subtracts. The brush supports adding/removing continuous strokes with diameter 1–512 px. Magnetic settings expose detection width, contrast, anchor frequency and All Layers. English, Japanese and Simplified Chinese are included.

[Done] Rust owns pending gesture geometry and the source edge map; WebView sends settings/actions only. Magnetic detection uses RGBA gradients from a maximum-1024-pixel composite preview, including color and alpha edges. Selection polygon/stroke geometry is shared by painting, fill, movement, native persistence and GPU clipping/outline. Preview does not mutate the live document. Commit validates document identity/revision and creates selection-only Undo/Redo history. Invalid/cancelled gestures leave the document unchanged. Legacy rectangular/elliptical native selections remain readable.

Verification: 644 workspace tests pass, including concave polygons, continuous strokes, add/subtract, translation, invalid input atomicity, native save/reload, edge detection and native pointer gestures. Four hardware GPU selection regressions pass on Apple M4/Metal, including CPU/GPU containment comparison for concave polygons and brush subtraction. TypeScript, Clippy (warnings denied), formatting, architecture boundaries and diff checks pass. macOS debug app build succeeds (142.18 MiB). Three-language settings dialogs for all four tools were exercised with bundled Playwright/Chrome; no page errors or horizontal overflow. Japanese magnetic settings screenshot was visually inspected. Browser plugin was unavailable. Temporary fixtures/server removed. Physical mouse interaction in the packaged app and performance benchmarks were not automated.

[Next] Soft selection/feathering and intersection mode; refinement of magnetic attachment on high-resolution and weak-edge images. Current masks have binary coverage and do not implement Photoshop's soft Selection Brush opacity.

[Later] Quick Selection, object/subject selection, Select and Mask, and reusable selection channels. Adobe bit-for-bit selection parity is not claimed.

Reference: https://helpx.adobe.com/ca/photoshop/using/selecting-lasso-tools.html
