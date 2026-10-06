# Paint Bucket QA — 2026-10-06

[Done] Paint workspace tool with Shift+G shortcut (G retains Gradient), click-to-fill, foreground color and three built-in transparent-background patterns (checker, stripes, dots). Pattern placement is anchored in document pixels. Tool settings include six supported blend modes, opacity, tolerance 0–255, anti-alias, contiguous and All Layers. Labels and help are available in English, Japanese and Simplified Chinese. Toolbar changes apply immediately; settings-dialog changes commit on Apply and are discarded on Cancel.

[Done] Rust worker owns the region search, source sampling, destination pixels and PNG encoding. Contiguous uses four-neighbor connectivity; disabling it fills all matching pixels in the destination selection. Color matching compares straight RGBA components against the clicked pixel, treating fully transparent pixels consistently. Anti-alias adds partial coverage to neighboring threshold-edge pixels without continuing the flood through them. All Layers samples the host's visible composite and modifies only the selected pixel target. The selection restricts both region traversal and destination edits. Rendering/large-tile color blending use the existing wgpu pipeline with CPU fallback; no full image data crosses JavaScript IPC.

[Done] Base/background and image-backed paint targets use one atomic history entry through the existing Rust pixel-edit path. No color, zero opacity, out-of-bounds/selection clicks and byte-identical output do not create an edit. The host checks target document, layer and revision before commit. Locked, alpha-locked, hidden and vector targets are rejected by the shared editable-pixel checks. Intermediate preview is coalesced through dirty tile updates.

Verification: 636 workspace tests pass. New coverage checks local/global matching, hard/anti-aliased tolerance edges, selection clipping, opacity, unchanged output, invalid input, document-anchored pattern repetition, base-layer Undo/Redo and All Layers on an empty image layer while preserving its source layer. TypeScript, Clippy (warnings denied), formatting, architecture boundaries and diff checks pass. macOS debug app build succeeds (141.17 MiB). Existing GPU blending tests cover the six reused modes and premultiplied alpha; a separate full-fill hardware benchmark was not performed.

Browser plugin unavailable: bundled Playwright/Chrome verified the real tool dialog in all three languages, opacity/tolerance input, contiguous/All Layers controls and pattern selection; no page errors or horizontal overflow. Japanese screenshot `/private/tmp/lp-bucket-ja.png` visually inspected. Temporary fixtures and the development server removed. Toolbar controls scroll horizontally when space is limited. Native pointer routing is covered by Rust tests; physical-device manual interaction was not automated.

[Next] Filling with locked transparency while preserving target alpha; editable tile-backed large documents; broader group/effect composite sampling comparisons.

[Later] Custom imported pattern libraries and additional Photoshop blend modes. Adobe bit-for-bit tolerance/edge parity is not claimed.

Reference: https://helpx.adobe.com/uk/photoshop/desktop/apply-painting-techniques/fill-objects-selections-layers/fill-paint-bucket-tool.html
