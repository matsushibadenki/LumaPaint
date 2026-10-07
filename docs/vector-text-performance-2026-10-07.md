# Vector and text performance: measured first changes

The attached improvement specification is being implemented in stages. The first changes added diagnostic coverage and fixed a measured text-cache eligibility bug. The follow-up below adds bounded retained text-frame GPU composition. The entire specification is not complete.

## Implemented

- [Done] Opt-in thread-local CPU counters/timers in `lumapaint-renderer::performance`. Set `LUMAPAINT_RENDER_METRICS=1` or `LUMAPAINT_RENDER_DEBUG=1` before launching. Both the render thread and macOS SVG worker emit separate snapshots. No mutex, timer clock read, or diagnostic-map allocation occurs when disabled. Timings are inclusive CPU/host durations; nested durations must not be summed as total frame time.
- [Done] Instrument SVG parsing including text shaping, rasterization including readback, text-frame preparation/composition, full-canvas CPU allocation, frame/parsed-text cache hits and misses, native journal validation/refits, spatial query and verification/build, native geometry/uniform uploads, SVG texture creation/write bytes, and queue submission/presentation host time. Native-ineligibility reasons identify the first incompatible feature of each object. These are eligibility counts, **not** a count of objects finally rasterized.
- [Done] Fix nested text detection for parsed-text retention. The upstream `Tree::has_text_nodes()` misses text below ordinary groups. The retained-memory accounting traversal now detects nested text directly while preserving the source equality check, image/filter/paint-server exclusions, entry limit and estimated 32 MiB budget. Fonts remain shared. No raster resolution, antialiasing, text content or positioning changes.
- [Done] Add a nested-text retention regression and retain fresh-vs-cached exact pixel comparisons for three languages, sizes, vertical layout, transforms and clipping. Extend the existing benchmark with 20 warm samples and median/p95/p99 plus isolated warm-phase diagnostics.

## Measurements

Command: `LUMAPAINT_RENDER_METRICS=1 cargo run --offline --locked -p lumapaint-renderer --features skia --example bench_text_resolution`.

Local macOS machine, Cargo **dev** profile (renderer/core opt-level 2), actual reported backend **Resvg**. These are renderer-only CPU timings, not Release results, GUI input latency, GPU execution times or FPS guarantees. Before/after averages below used the original three-sample benchmark; a subsequent 20-sample run is reported separately. No GPU transfer occurs in this CPU-only benchmark.

| Warm raster workload | Before average | After average |
| --- | ---: | ---: |
| 5 lines, 2048 × 2048 | 16.0 ms | 4.2 ms |
| 45 lines, 2048 × 2048 | 104.8 ms | 36.2 ms |
| 45 lines, 1024 × 1024 | 75.8 ms | 16.6 ms |
| 45 lines, 512 × 512 | 64.5 ms | 8.2 ms |

Subsequent 45-line warm 20-sample results:

| Size | Median | p95 | p99 | Parsed-cache hits | SVG parses |
| --- | ---: | ---: | ---: | ---: | ---: |
| 2048 | 35.08 ms | 37.92 ms | 38.72 ms | 20 | 0 |
| 1024 | 15.65 ms | 15.72 ms | 15.92 ms | 20 | 0 |
| 512 | 8.08 ms | 8.20 ms | 8.26 ms | 20 | 0 |

The initial full benchmark had 56 parses, no parsed-cache hits; the same three-sample workload after the fix had 28 parses and 28 hits. Cold parsing and editing distinct content still require shaping. In the two-frame single-edit test, cached preparation remained roughly 45 ms (45.8 before, 44.7 after); this change does not establish an editing speedup. Its four preparations still allocated four full 2048² canvases (64 MiB aggregate allocations), rasterized five frame versions and reused three frames. CPU composition remains; caching parsing cannot remove it.

Validation: renderer unit/integration tests passed (203 tests, 39 intentionally ignored GPU/manual tests); the native cubic GPU regression was run explicitly and passed, including resident-geometry reuse and compatibility-image checks. Renderer all-targets Clippy with `-D warnings`, macOS host `cargo check -p lumapaint`, formatting and diff whitespace checks passed.

## Remaining specification work

- [Next] Finish phase 1 coverage: remaining core edit paths, all buffer/texture creation/write sites, draw-pass CPU encoding, and complete fallback routing. Current `*_uploaded_bytes` counters are scoped to their named SVG/native path, not a global transfer total; GPU timestamps and memory/RSS remain unmeasured.
- [Next] Extend phases 2–3 beyond the independent text-frame subset implemented below: overlapping frames, layer opacity, masks/effects and dependent groups need correct GPU composition. Preserve exact same-backend comparisons. Fractional translations and moves introducing overlap still use compatibility preparation. Native retained glyph/path geometry remains unfinished.
- [Next] Measure text/path move, edit, pan and zoom with representative 100/1,000/10,000-object scenes. Collect application input-to-present latency and upload/allocation counts separately from this raster benchmark.
- [Later] Phases 4–6: enable normal-zoom native vectors only after quality gates; extend native strokes, joins/caps, gradients and clips with explicit fallback reasons.
- [Later] Phases 7–8: remove remaining broad journal/spatial walks, test 100,000/1,000,000 objects within the application capacity contract, and perform Release warm-up plus 60-second × 5 runs with deadline miss rates. Twenty samples are not a substitute for that acceptance test or Adobe-output comparison.

`performance::take()` drains the calling thread only. A worker snapshot must be captured on that worker. Frame logs include work since the previous snapshot; diagnostic names explicitly distinguish host timings from GPU elapsed time. The debug flag currently enables statistics/reason output; it does not add a visual fallback-color overlay.

## Follow-up: independent text frames on the GPU

- [Done] Add `FrameRasterCache::prepare_display_layer`, returning shared cropped frame payloads instead of a CPU document-sized RGBA buffer for eligible layers. Integrate it into both the macOS SVG worker and synchronous text preparation. Export and compatibility APIs keep their existing full-pixel contract.
- [Done] Keep per-frame GPU textures and rectangle buffers. Exact source, object ID, rectangle and document dimensions guard reuse; an edited frame uploads its cropped pixels while unchanged frames retain their resource. Results are generation-checked before installation.
- [Done] Copy frame textures into a retained full-sized GPU layer texture, then present with the previous layer sampling. Directly scaling independent quads produced 1-level sampling differences at 170%; that approach was replaced before completion. GPU-to-GPU copies preserve the encoded bytes without blending or readback. CPU full-canvas allocation/composition is removed for this subset; **a full-sized GPU target is still retained**. The initial version cleared that target and copied all frames on every recomposition; the local-update follow-up below replaces this work for warm targets.
- [Done] Require canonical all-text source, at most 64 frames, layer opacity 1, normal blending, no dependent groups/clips, contained frames and disjoint padded raster rectangles. Keep a transparent texel guard at frame edges. Overlap, empty layers, unsupported structure, outside-canvas content and layer effects keep compatibility rendering. Individual frame text clipping is already rasterized by the same renderer.
- [Done] Account frame plus GPU composition payload against the object-cache 64 MiB retention budget. When retaining frames exceeds the available budget, compose on the GPU but discard the independent GPU frame cache afterward; the composed layer remains in the existing SVG cache. This is not a global renderer/VRAM budget and transient GPU allocations remain separate.
- [Done] Preserve exact-selection tagging for individual frames. Contained, disjoint integer-coordinate committed translations/Undo/Redo reuse textures and compose their updated rectangles on the GPU. Fractional/outside/overlapping committed moves invalidate this subset rather than copying to an invalid position. The source-identity follow-up below removes the composed-image source-string copy for supported committed text moves.

### Follow-up measurements and validation

Same dev-profile CPU-only example, two frames on a 2048² document, edit one frame three times. Isolated new-path diagnostics assert zero `full_canvas_allocations` and zero `text_frame_cpu_composite` duration. Each edit rasterizes one frame and hits the unchanged frame once. The previous path allocates a 16,777,216-byte CPU canvas per preparation; final new-path frame payload is 5,352,024 bytes total (about 68% less), including the unchanged frame. This is **prepared pixel payload**, not a measured total application/GPU transfer count; the installer can additionally reuse the unchanged GPU frame.

The combined example reports approximately 43.9 ms cached full preparation and 15.9 ms retained preparation, but runs the retained path after other paths have warmed the shared parsed-text cache. These timings are not an unbiased before/after speedup comparison. Application input latency, Release 60-second × 5 runs, GPU elapsed time and VRAM/RSS remain unmeasured.

CPU renderer tests passed (205 tests). New regression covers exact full-pixel reconstruction, unchanged-frame sharing, overlap/layer-opacity fallback, and Undo/Redo without extra rasterization. The real GPU test compares composed and full-layer presentation exactly at 100%, 50% and 170%, including clearing previously occupied target pixels. Existing CPU-upload and shared-Metal object movement GPU regressions also passed. Host compilation and all-targets Clippy with warnings denied passed.

## Follow-up: local GPU clearing and copies

- [Done] Plan changed, moved, inserted and removed frame rectangles using exact frame ID, source and integer bounds. Clear old/new affected rectangles before copying new changed frames. Duplicate identical rectangles are cleared once. Unchanged frames are excluded from both clearing and copying. This depends on the existing disjoint-frame eligibility contract; overlapping/dependent layers retain compatibility rendering.
- [Done] Apply the plan to worker-result installation and supported committed integer translations/Undo/Redo. New/resized/missing composition targets still receive a full clear/copy. A missing independent GPU cache also requires full initialization.
- [Done] Clear local rectangles by copying from a GPU zero buffer, with 256-byte row alignment and chunking. The 4 MiB buffer is allocated lazily per renderer and retained for subsequent updates. No CPU zero image, pixel upload or GPU readback is required. This scratch allocation is separate from the 64 MiB object-cache retention accounting; a global GPU memory budget remains unfinished.
- [Done] With no dirty rectangles or changed frames, return before allocating the scratch buffer, creating an encoder or submitting GPU commands.

The real GPU regression exercises the production update wrapper: move one frame by 32 pixels, remove it, restore it, leave the second frame untouched, and compare against full-layer reference pixels at 50%, 100% and 170%. It also checks the unchanged/no-op path allocates no scratch buffer. A deliberately smaller 32 KiB test zero buffer exercises row chunking. All image comparisons are exact.

For this **960 × 640, two-frame GPU regression**, the prior full-clear path cleared 2,457,600 logical RGBA bytes and copied 112,320 frame bytes per update. Moving one frame now clears 112,320 bytes (old plus new rectangle, approximately **95.4% less**) and copies 56,160 bytes (**50% less**). Removing it clears 56,160 bytes and copies zero frame bytes. Every partial update records zero full clears. These are logical GPU texel-operation byte counts, not CPU upload volume, measured hardware bus transactions or GPU elapsed time. Host encoding timings are diagnostic only; application FPS/input latency still require the outstanding benchmark protocol.

Validation: 206 CPU renderer unit/integration tests, the expanded real GPU exact-comparison regression, host compilation and all-targets Clippy with warnings denied pass. Remaining priorities include dependent text composition, fractional moves, native stroke/glyph geometry, complete profiling coverage and representative large-scene Release/application measurements.

## Follow-up: source identity and shared core profiling

- [Done] Share thread-local diagnostics between core and renderer. Instrument committed/preview vector moves, text edits, horizontal/vertical reflow and nonempty scene journal updates. Timings remain inclusive host durations; they are not GPU timing and must not be added together. Other core mutation routes remain to be instrumented. The core text-preview example prints snapshots when metrics are enabled.
- [Done] Validate retained composed text images with the runtime journal instance, layer source pointer/length and geometry/transform/style/visibility/structure generations. Supported integer text translations no longer clone the entire layer SVG into the composed-image cache. Source tokens are runtime-only and never dereferenced. Prepared source ownership still moves into the object cache; small lookup allocations and other SVG copies remain. Full compatibility-image installs clear the token.
- [Done] Regression checks accept a composed image with no retained source string and reject stale move/Undo/Redo state and a foreign document clone. Existing real-GPU local composition tests retain exact full-render comparisons at 50%, 100% and 170%.

Validation: core and renderer tests passed (485 unit/integration tests; 41 manual/GPU tests ignored in the standard run). The opt-in core example emitted text-edit and journal counters. This does not establish application latency or FPS improvements. Full GPU transfer instrumentation, dependent text composition, native geometry coverage and Release application benchmarks remain unfinished.
