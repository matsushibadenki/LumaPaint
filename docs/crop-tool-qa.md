# Crop tool QA — 2026-10-06

[Done] Crop is a common tool for editable documents. C selects it; drag creates a pending crop area, corner/edge handles resize, inside moves; Enter or Apply crop commits, Escape/Cancel discards. Double-click/reclick opens numeric settings, prefilled from pending bounds when available. Numeric preview does not mutate the document. Bounds stay inside the existing canvas and are rounded outwards to whole document pixels on commit.

Core crop keeps exterior artwork, translates all layers including locked/hidden content, brush points and their selection clips, vector objects, imported SVG sources, live compound operands, saved paths, guides and ruler origin. Crop dimensions are carried only by crop history entries; unrelated vector Undo does not restore document settings. Active-page sizing leaves inactive pages unchanged. Native format preserves exterior content; normal export clips to its canvas as before.

Verification: full workspace tests, TypeScript, Clippy, formatting/boundary checks and macOS debug app build. Added regression checks for brush translation and atomic failed/no-op requests, native round trip, off-canvas vector preservation, imported SVG/XML handling, active-page dimensions, pending rectangle drag/corner/move geometry, and exact SVG cropped pixel rows versus the original region plus Undo image equality.

Browser plugin unavailable; bundled Playwright/Chrome tested the real crop ToolSettingsDialog in a temporary fixture. Full-canvas defaults, four numeric inputs, invalid oversized bounds blocked, Japanese/English/Simplified Chinese, no page errors or Vite overlay. Screenshot `/private/tmp/lp-crop-settings.png` visually inspected: labels, inputs, preview and buttons fit. Native pointer/keyboard automation and GPU pixel comparisons were not performed. Fixtures removed after verification.

Limitations: tile-backed read-only documents keep this tool disabled. Crop expansion beyond the original canvas, rotation/perspective correction, aspect-ratio presets and destructive removal of exterior pixels are subsequent work. This implementation retains exterior artwork.
