# Brush settings

The options bar, Brush panel and Brush/Eraser tool settings share the same brush state. Labels and blend-mode names support Japanese, English and Simplified Chinese.

| Control | Range / default | Behavior |
| --- | --- | --- |
| Opacity | 0–100% / 100% | Caps coverage for one continuous stroke. A subsequent stroke can build on the previous stroke. |
| Flow | 0–100% / 100% | Scales each dab's optical density; a stationary hard-tip click at 50% flow produces 50% coverage before opacity and alpha. Repeated passes build coverage up to the stroke's opacity × alpha cap. |
| Smoothing | 0–100% / 0% | Stabilizes input points before the existing curve/dab sampler. Higher values reduce jitter with more pointer lag; the maximum stabilization distance is 24 document pixels. |
| Alpha | 0–100% / 100% | Transparency of the brush color. Independent from layer alpha lock. Opacity 50% and alpha 50% cap the stroke at 25%. |
| Blend mode | 29 modes / Normal | Combines the brush color with existing pixels in the edited layer. Eraser uses Clear. |

Groups follow the supplied Photoshop reference:

- Normal, Dissolve, Behind, Clear
- Darken, Multiply, Color Burn, Linear Burn, Darker Color
- Lighten, Screen, Color Dodge, Linear Dodge (Add), Lighter Color
- Overlay, Soft Light, Hard Light, Vivid Light, Linear Light, Pin Light, Hard Mix
- Difference, Exclusion, Subtract, Divide
- Hue, Saturation, Color, Luminosity

Dissolve uses stable document-coordinate noise so refreshing, replaying or undoing a stroke does not change its pattern. Layer alpha lock still prevents changing pixel alpha; Behind and Clear do not paint an alpha-locked layer.

## Rendering and compatibility

Brush state and retained strokes remain in Rust. Brush sampling applies flow and smoothing; the Rust tile compositor applies the stroke opacity, color alpha and blend mode. The existing wgpu presentation consumes dirty tiles. Pixel payloads do not cross the WebView bridge.

The same compositor serves live previews, committed pixels, thumbnails and retained-stroke replay. Existing documents without these fields load with the defaults above. Undo/Redo and native document serialization retain the new parameters. Choosing a tip preset preserves these settings.

Pixel masks retain grayscale coverage with compact runs, including partial brush coverage, inverted masks, selection clipping and moved masks. CPU and GPU mask readers consume the same runs. Vector masks remain geometric. Selected-channel edits apply grayscale blending to the selected component and preserve the unselected color components (premultiplied RGB is rescaled when editing alpha).

## Validation

Automated coverage includes old/new brush serialization, range validation, blend arithmetic, stroke opacity × alpha, flow buildup, smoothing, live/committed agreement, history, mask grayscale persistence and selected-channel isolation. Browser component checks cover all 29 modes, controls, toolbar/panel synchronization, presets, Eraser behavior, three locales and 1440/1024/390-pixel layouts.

References: [Adobe painting tools](https://helpx.adobe.com/photoshop/using/painting-tools.html), [Adobe blending-mode descriptions](https://helpx.adobe.com/ca/photoshop/desktop/repair-retouch/adjust-light-tone/blending-mode-descriptions.html). These references define the intended behavior and UI vocabulary; this implementation does not claim pixel-identical output with Adobe applications.

Verified locally on macOS: production frontend build; workspace tests (888 passed, 56 explicitly ignored); final core/renderer regression after mask-boundary correction (549 passed, 48 explicitly ignored); explicit GPU/CPU mask comparison; workspace Clippy with warnings denied; formatting and dependency-boundary checks. Browser checks use the actual React components in a temporary harness, not a packaged native-window end-to-end run. Stylus feel and native Windows/Linux behavior remain manual-device checks.

## Natural media and paper — 2026-10-22

The Brush panel includes paper and material controls in Japanese, English and Simplified Chinese. Paper has smooth, cold-press, rough, canvas and washi relief; scale 0.25–8, depth 0–1 and RGB color. Apply commits a single undoable document setting. Existing documents default to disabled. Paper affects white backgrounds, color sampling, thumbnails and raster export while preserving transparent backgrounds. SVG/PDF use the existing raster fallback and report `svg.paper_rasterized`; this preserves appearance but flattens editable vectors in those exports. Native project files retain editable content and settings.

Six procedural deposition models provide graphite, charcoal, pastel, watercolor, gouache and oil. Loading controls contact with relief, granulation changes pigment breakup, wetness attenuates deposits for wet media, and watercolor edge strengthens the radial rim of each dab. Oil adds elongated bristle marks. These are artistic approximations, not fluid/pigment/impasto physics. Time-dependent drying, diffusion, mixing with existing wet pigment, whole-stroke watercolor edge pooling and oil thickness lighting are not implemented. Relief is procedural rather than scanned paper. Watercolor can be combined with the existing Smudge tool, but that does not constitute wet-pigment mixing.

`PaperSettings` belongs to Rust Document state; applying it validates before mutation and creates a compact history entry. Every retained stroke captures paper at pointer-down, including separate pixel-layer paint workspaces. Brush material parameters persist with strokes. Later paper changes alter the visible substrate, not old pigment deposits. UI sends only settings. wgpu evaluates deposition in the brush shader and substrate in the background shader; native tile replay uses the same arithmetic. Document coordinates keep texture fixed across zoom and dab placement. Erasers ignore material deposition. Normal brushes remain unchanged when simulation is disabled.

Browser QA uses the production controls in an isolated localhost:1422 fixture with mocked IPC. Browser plugin not available; bundled Playwright/installed Chromium used. Six media choices, paper apply payload, disabled states and all three locales passed; 360px layout has no horizontal overflow, runtime errors or framework overlay. Screenshot `/tmp/lumapaint-natural-media-panel.png` is temporary evidence. This does not verify physical native pen input or pressure feel. The packaged app was not restarted. The native paper command currently supports macOS standard documents; standalone tiled documents and other OS hosts remain Pending.

References: [requested watercolor tutorial](https://tips.clip-studio.com/ja-jp/articles/5637), [CLIP STUDIO official brush detail controls](https://tips.clip-studio.com/ja-jp/articles/563), [Painter paper interaction](https://product.corel.com/help/Painter/540235477/707000/EN/Doc/Watercolor_Brushes_and_Paper_Texture_Interaction.html), [Painter paper texture](https://product.corel.com/help/Painter/540215550/Main/EN/Win-Documentation/Corel-Painter-Using-paper-to-create-texture.html). Source brush assets and proprietary algorithms were not copied.

Final validation: core 393 passed, 2 explicitly ignored. New natural-media persistence/history/parameter tests (3), paper alpha/compositing test (1), and vector-export loss-report test (1) passed. Four shader-validation tests and four existing brush-setting tests passed. An explicit Apple/Metal hardware regression draws all six media, compares sampled pixels with native tile replay (maximum tolerated channel difference 4/255) and checks that material deposits differ. It passed again after smoothing coarse paper relief and fixing zero-loading behavior; the generated sample is `/tmp/lumapaint-natural-media-gpu.png` (top to bottom: graphite, charcoal, pastel, watercolor, gouache, oil). Frontend production build, type checking, core dependency boundaries, formatting, diff checks and warning-denied Clippy for core/renderer/formats/macOS host pass. The build retains the existing large-chunk warning. Browser interaction QA passed again after tightening control-row spacing. No release input-latency benchmark or physical pen run was performed.
