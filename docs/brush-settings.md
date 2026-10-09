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
