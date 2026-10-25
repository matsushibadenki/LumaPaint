# Native tiled selection refinement — 2026-10-25

Release acceptance used the embedded Tauri UI, actual WKWebView and Apple M4/Metal in a separate owned QA bundle. Browser/IPC mocks were not used. The window was 1200×800 with an 800×574 native viewport at scale 1. All compilation and tests finished before timing. The fixture is a blank 4096×2048 document, not production artwork.

| Selection state | Retained samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Geometric selection, 1×120px hole | 16 | 10.651ms | 13.622ms | 13.622ms |
| Native RLE after edge shift −1, 3×122px hole | 16 | 10.556ms | 17.152ms | 17.152ms |

These measure native handler start → present API call. They exclude OS queue delay, GPU completion and physical display. Each accepted trial contains 10 native zoom-in/out cycles (20 presentations) at window (490,440), with the first four presentations discarded. Z/Option-Z switch tools; native accessibility checks assert the active tool and 100%→125%→100% on every cycle. The accepted order was refined mask B2, then geometric selection A3 restored with Undo. Both first/latest handler metrics agree in these trials. Timing instrumentation was enabled equally for both conditions. Invalid GPU timestamp samples do not affect these host-clock values.

The medians are similar; the mask trial has a higher p95. This small within-build comparison establishes neither an old/new code speedup nor a statistically reliable regression. Continuous stylus work, real artwork, worker contention, a sustained multi-trial distribution and Windows/Linux remain unmeasured. Earlier exploratory and automation-interrupted captures are excluded. The zoom field can consume shortcuts while focused; accepted trials explicitly restore canvas focus and validate all transitions.

Native Refine Shift Edge=−1 preserved an identity-scale 4096×2048 raster with 123 runs. Saved data contains a 3×122px hole at (1999,999), with opaque immediate neighbors. Undo/save restored the original two geometric regions; Redo/save restored the native raster. Quick Mask at 100% visibly shows the three-pixel protected strip. The Japanese information tooltip describes the new native-size limit. Screenshot: `/tmp/lumapaint-refine-release-result.png`. The QA app quit with exit code 0 and its temporary bundle was removed.

A separate input-edit observation: typing a minus sign into the controlled numeric field reset the intermediate value; keyboard Down steps correctly reached −1. Numeric draft editing remains a follow-up and is not included in latency timing.

[Machine-readable evidence](input-latency-selection-refine-2026-10-25.json) records source/binary hashes, raw host samples, zoom-state proofs and native file assertions. Logs and the owned fixture remain under `/tmp/lumapaint-refine-release-*`. The Release build used `cargo build -p lumapaint --release --features tauri/custom-protocol --locked --offline` with the QA identity override. No synthetic Rust input injection was used.
