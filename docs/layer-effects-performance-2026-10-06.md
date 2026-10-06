# Layer effects performance — 2026-10-06

[Done] Batch processing prepares exposure/temperature/tint/tonal adjustments for the 256 possible input channel bytes. Table entries retain f32 intermediates; they are not RGB8/3D lookup approximations. It also prepares curve tangents, mixer/grading activation flags and grading tint colors. Saturation, HSL interactions and final rounding keep the reference evaluation order. Disabled layers skip preparation. Renderer samples below 64 pixels keep the direct path, so single-pixel color sampling does not build a table.

[Done] Prepared evaluation is used in premultiplied render buffers, straight-alpha tile composition, thumbnails and PSD/PSB effect baking. Alpha and original source/model settings are retained; save format and UI interaction are unchanged.

[Done] Workspace viewport caches retain raw and adjusted pixels separately. Effects and opacity changes, including Undo/Redo, reuse raw pixels rather than parsing/rasterizing unchanged SVG/text again. Existing processed storage is reused when reapplying effects. Geometry changes still update local tiles (including raw pixels), with full invalidation for viewport/size/history changes and unsupported SVG changes. A failed refresh invalidates the cache key. Retained CPU raw pixels are bounded to 32 MiB and adjusted pixels to 32 MiB across visible layers; this 64 MiB bound excludes transient prepared upload payloads, SVG strings and GPU textures.

## Release microbenchmark

Run `cargo run -p lumapaint-core --example bench_layer_effects --release --locked`.

Same local macOS environment, 262,144 deterministic opaque RGB pixels per case; one warm-up and five measured runs, median. Prepared time includes plan construction. Reference/prepared checksums must agree, with byte-by-byte comparisons performed separately in tests. Rasterization, compositor, IPC and display latency are excluded; these results do not establish an application FPS or total interaction speedup.

| Adjustments | Reference (ms) | Prepared (ms) | Pixel-processing speedup |
| --- | ---: | ---: | ---: |
| Tone/exposure/temperature | 39.974 | 10.502 | 3.81× |
| Tone + smooth curves | 53.651 | 16.855 | 3.18× |
| Tone + curves + saturation + mixer + grading | 80.188 | 31.441 | 2.55× |

## Verification

Core byte comparisons cover 12 configurations, including extreme exposure/tonal controls, smooth/linear curves, HSL mixing and grading; 8,192 deterministic colors plus all grayscale bytes per configuration, with transparent and partially transparent alpha. Renderer comparisons cover premultiplied 1/64/8,192-pixel batches. Cache tests verify no new raster calls for effect, opacity and Undo/Redo changes; raw-cache reuse after clipped geometry tile updates matches full rendering within the existing one-byte tile-edge tolerance. Existing native storage, history and export tests remain applicable.

Final verification: all 662 workspace tests pass, including PSD/PSB export coverage. Workspace Clippy (warnings denied), formatting, dependency boundary checks and diff checks pass. macOS debug application bundle builds successfully (143.17 MiB); the running application was not restarted. No GPU shaders or UI controls were changed; hardware GPU tests and physical-device interaction were not repeated for this CPU/cache change.

[Done] Subsequent renderer GPU batch computation and CPU fallback are documented separately in [GPU verification](layer-effects-gpu-qa-2026-10-06.md). The CPU measurements above describe the earlier CPU/cache change.

[Next] GPU tile composition and export baking, histogram reuse, and end-to-end input/display latency measurements for large image and text layers.

[Later] A common CPU/GPU cache budget and long-session peak-memory measurements.
