# Clone Stamp QA — 2026-10-06

[Done] The paint workspace has Clone Stamp (S), with Option/Alt-click source selection, aligned offsets across strokes and unaligned restart at the selected source. Rust owns each window's settings and source/document identity. Native macOS pointer events capture every stroke sample; a worker prepares an immutable source once per stroke and publishes coalesced dirty-tile previews. Stamping supports the base pixel layer and image-backed paint layers. Vector layers are sampling sources through Current & Below or All Layers; stamping requires an unlocked, visible pixel target with alpha lock disabled.

[Done] Diameter, hardness, Normal/Multiply/Screen/Overlay/Darken/Lighten, opacity, flow, sample scope, alignment, angle, roundness and size/opacity pressure toggles. Settings have English, Japanese and Simplified Chinese labels. Toolbar settings apply immediately; the settings dialog applies its changes on confirmation, and Cancel preserves the prior settings. Selection limits the destination, not the sampling source. Immutable source pixels prevent feedback when the brush overlaps its source. Distance-based dabs keep flow independent of pointer event frequency. Coverage is capped by per-stroke opacity. Transparent sampling does not erase the destination.

[Done] Color blending batches of at least 4,096 pixels prefer a wgpu compute pipeline, with CPU fallback when GPU initialization, limits or readback prevent GPU use. Brush coverage, source interpolation and PNG encoding run on the Rust worker. Preview composition/upload uses the existing wgpu tile pipeline; full raster data never crosses JavaScript IPC. GPU comparison covers all six modes and premultiplied alpha with at most one byte of rounding difference. The direct test passed on the host GPU outside the sandbox; the sandbox run exercised automatic CPU fallback.

Verification: 628 workspace tests pass, including source-required behavior, document switch rejection, aligned/unaligned offsets, base/image-layer commits and exact document Undo/Redo, selection boundaries, immutable source, opacity cap, flow sampling consistency and alpha/mode blending. TypeScript, Clippy with warnings denied, Rust formatting, architecture boundary and diff checks pass. macOS debug app build succeeds (140.53 MiB).

Browser plugin unavailable: bundled Playwright with Chrome tested the real settings dialog in Japanese, English and Simplified Chinese. Numeric opacity/flow inputs, sample selection and alignment checkbox work; no page errors or horizontal overflow. Japanese screenshot `/private/tmp/lp-stamp-ja.png` visually inspected after compact spacing and right-aligned numeric/unit adjustments. Temporary fixtures and the development server were removed. Native input is covered by Rust pointer-route tests; manual pen/device performance measurements were not performed.

[Next] Tile-backed read-only document editing, selection/locking-aware larger-document caches, further composite sampling coverage for complex groups/effects.

[Later] Photoshop's Clone Source panel (multiple sources, source scaling/mirroring and overlay controls), custom brush-tip libraries and additional blend modes. This does not claim complete Photoshop feature parity.

Reference: https://helpx.adobe.com/uk/photoshop/desktop/repair-retouch/heal-clone/retouch-images-with-the-clone-stamp-tool.html
