# Vector and text performance: measured first changes

## Current specification status (2026-10-09)

This table supersedes earlier chronological remaining-work notes below. The specification is **not complete**; completed subsets must not be reported as complete phases.

| Phase | Current result | Remaining acceptance work |
| --- | --- | --- |
| 1: diagnostics | 🟢 [Done] Opt-in host timings, Rust heap profiling, scoped GPU resources/copies/readback, Skia cache usage, host RSS and correlated display/compute GPU timestamps | 🟠 [Next] Complete worker/mutation coverage, complete live/driver resource memory, Skia-internal GPU timing, cross-platform validation and physical input-to-display measurement; handler-to-present-call diagnostics are implemented |
| 2: full-canvas text composition | 🟢 [Done] Eligible contained canonical text uses independent crops or immutable composite tiles; tiled display no longer materializes a contiguous CPU image | 🟠 [Next] Groups, clips, masks/effects and mixed-content eligibility; compatibility paths still have full canvases |
| 3: retained text | 🟢 [Done] Independent retained GPU crops/local target updates and overlapping tiles; Glyph Atlas masks, shaped instances and opt-in application display pilot | 🟠 [Next] Default-enable quality/GUI gates, fractional zoom/drag, variable/color fonts, decorations and universal transform-only moves |
| 4: native normal zoom | 🟢 [Done] Qualified opaque straight strokes and single pixel-aligned opaque rectangular fills use native GPU rendering at normal zoom; six-zoom pixel comparisons | 🟠 [Next] Cubic fills and other appearance still need edge-quality corrections before normal-zoom routing |
| 5: native strokes | 🟢 [Done] Bounded opaque axis-aligned straight strokes, butt/square caps and dashes; exact rational-conic storage and round undashed straight caps above 150%; retained geometry | 🟠 [Next] Curves/polylines and round joins remain quality-gated; normal-zoom round caps, overlapping/transparent strokes, mixed fill/stroke and general transforms |
| 6: gradients/clipping | 🟢 [Done] Retained GPU linear/radial paints, nested clipping and isolated layer opacity/order; compatibility pixel comparisons pass | Complete within the bounded native geometry contract below; unsupported geometry/appearance retains compatibility rendering. Full Adobe comparisons remain phase 8. |
| 7: incremental Scene | 🟢 [Done] Native transform refits reuse local bounds; idle synchronization skips source walks and retained viewport queries; unchanged layers skip journal copies; structural removals prune cached IDs locally; existing bounds membership changes refit locally | 🟠 [Next] Extend journal-based reuse to every rendering route; local object insertion/deletion BVH updates and remaining compatibility/mixed-content verification scans |
| 8: scale/performance gates | 🟢 [Done] Release spatial-index query/refit example up to one million entries | 🟠 [Next] End-to-end application workloads, capacity changes, 60-second × 5 runs, GPU/RSS/upload budgets; 🟢 [Done] First Illustrator 2026 geometry export fixture and measured compatibility comparison; 🟠 [Next] native/glyph/color-managed Adobe comparisons |

The attached improvement specification is being implemented in stages. The first changes added diagnostic coverage and fixed a measured text-cache eligibility bug. The follow-up below adds bounded retained text-frame GPU composition. The entire specification is not complete.

## Implemented

- 🟢 [Done] Opt-in thread-local CPU counters/timers in `lumapaint-renderer::performance`. Set `LUMAPAINT_RENDER_METRICS=1` or `LUMAPAINT_RENDER_DEBUG=1` before launching. Both the render thread and macOS SVG worker emit separate snapshots. No mutex, timer clock read, or diagnostic-map allocation occurs when disabled. Timings are inclusive CPU/host durations; nested durations must not be summed as total frame time.
- 🟢 [Done] Instrument SVG parsing including text shaping, rasterization including readback, text-frame preparation/composition, full-canvas CPU allocation, frame/parsed-text cache hits and misses, native journal validation/refits, spatial query and verification/build, native geometry/uniform uploads, SVG texture creation/write bytes, and queue submission/presentation host time. Native-ineligibility reasons identify the first incompatible feature of each object. These are eligibility counts, **not** a count of objects finally rasterized.
- 🟢 [Done] Fix nested text detection for parsed-text retention. The upstream `Tree::has_text_nodes()` misses text below ordinary groups. The retained-memory accounting traversal now detects nested text directly while preserving the source equality check, image/filter/paint-server exclusions, entry limit and estimated 32 MiB budget. Fonts remain shared. No raster resolution, antialiasing, text content or positioning changes.
- 🟢 [Done] Add a nested-text retention regression and retain fresh-vs-cached exact pixel comparisons for three languages, sizes, vertical layout, transforms and clipping. Extend the existing benchmark with 20 warm samples and median/p95/p99 plus isolated warm-phase diagnostics.

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

- 🟠 [Next] Finish phase 1 coverage: remaining core edit paths, all buffer/texture creation/write sites, draw-pass CPU encoding, and complete fallback routing. Current `*_uploaded_bytes` counters are scoped to their named SVG/native path, not a global transfer total; GPU timestamps and memory/RSS remain unmeasured.
- 🟠 [Next] Extend phases 2–3 beyond the independent text-frame subset implemented below: overlapping frames, layer opacity, masks/effects and dependent groups need correct GPU composition. Preserve exact same-backend comparisons. Fractional translations and moves introducing overlap still use compatibility preparation. Native retained glyph/path geometry remains unfinished.
- 🟠 [Next] Measure text/path move, edit, pan and zoom with representative 100/1,000/10,000-object scenes. Collect application input-to-present latency and upload/allocation counts separately from this raster benchmark.
- 🔴 [Later] Phases 4–6: enable normal-zoom native vectors only after quality gates; extend native strokes, joins/caps, gradients and clips with explicit fallback reasons.
- 🔴 [Later] Phases 7–8: remove remaining broad journal/spatial walks, test 100,000/1,000,000 objects within the application capacity contract, and perform Release warm-up plus 60-second × 5 runs with deadline miss rates. Twenty samples are not a substitute for that acceptance test or Adobe-output comparison.

`performance::take()` drains the calling thread only. A worker snapshot must be captured on that worker. Frame logs include work since the previous snapshot; diagnostic names explicitly distinguish host timings from GPU elapsed time. The debug flag currently enables statistics/reason output; it does not add a visual fallback-color overlay.

## Follow-up: independent text frames on the GPU

- 🟢 [Done] Add `FrameRasterCache::prepare_display_layer`, returning shared cropped frame payloads instead of a CPU document-sized RGBA buffer for eligible layers. Integrate it into both the macOS SVG worker and synchronous text preparation. Export and compatibility APIs keep their existing full-pixel contract.
- 🟢 [Done] Keep per-frame GPU textures and rectangle buffers. Exact source, object ID, rectangle and document dimensions guard reuse; an edited frame uploads its cropped pixels while unchanged frames retain their resource. Results are generation-checked before installation.
- 🟢 [Done] Copy frame textures into a retained full-sized GPU layer texture, then present with the previous layer sampling. Directly scaling independent quads produced 1-level sampling differences at 170%; that approach was replaced before completion. GPU-to-GPU copies preserve the encoded bytes without blending or readback. CPU full-canvas allocation/composition is removed for this subset; **a full-sized GPU target is still retained**. The initial version cleared that target and copied all frames on every recomposition; the local-update follow-up below replaces this work for warm targets.
- 🟢 [Done] Initially require canonical all-text source, at most 64 frames, layer opacity 1 (extended below), normal blending, no dependent groups/clips, contained frames and disjoint padded raster rectangles. Keep a transparent texel guard at frame edges. Overlap, empty layers, unsupported structure, outside-canvas content and layer effects keep compatibility rendering. Individual frame text clipping is already rasterized by the same renderer.
- 🟢 [Done] Account frame plus GPU composition payload against the object-cache 64 MiB retention budget. When retaining frames exceeds the available budget, compose on the GPU but discard the independent GPU frame cache afterward; the composed layer remains in the existing SVG cache. This is not a global renderer/VRAM budget and transient GPU allocations remain separate.
- 🟢 [Done] Preserve exact-selection tagging for individual frames. Contained, disjoint integer-coordinate committed translations/Undo/Redo reuse textures and compose their updated rectangles on the GPU. Fractional/outside/overlapping committed moves invalidate this subset rather than copying to an invalid position. The source-identity follow-up below removes the composed-image source-string copy for supported committed text moves.

### Follow-up measurements and validation

Same dev-profile CPU-only example, two frames on a 2048² document, edit one frame three times. Isolated new-path diagnostics assert zero `full_canvas_allocations` and zero `text_frame_cpu_composite` duration. Each edit rasterizes one frame and hits the unchanged frame once. The previous path allocates a 16,777,216-byte CPU canvas per preparation; final new-path frame payload is 5,352,024 bytes total (about 68% less), including the unchanged frame. This is **prepared pixel payload**, not a measured total application/GPU transfer count; the installer can additionally reuse the unchanged GPU frame.

The combined example reports approximately 43.9 ms cached full preparation and 15.9 ms retained preparation, but runs the retained path after other paths have warmed the shared parsed-text cache. These timings are not an unbiased before/after speedup comparison. Application input latency, Release 60-second × 5 runs, GPU elapsed time and VRAM/RSS remain unmeasured.

CPU renderer tests passed (205 tests). New regression covers exact full-pixel reconstruction, unchanged-frame sharing, overlap/layer-opacity fallback, and Undo/Redo without extra rasterization. The real GPU test compares composed and full-layer presentation exactly at 100%, 50% and 170%, including clearing previously occupied target pixels. Existing CPU-upload and shared-Metal object movement GPU regressions also passed. Host compilation and all-targets Clippy with warnings denied passed.

## Follow-up: local GPU clearing and copies

- 🟢 [Done] Plan changed, moved, inserted and removed frame rectangles using exact frame ID, source and integer bounds. Clear old/new affected rectangles before copying new changed frames. Duplicate identical rectangles are cleared once. Unchanged frames are excluded from both clearing and copying. This depends on the existing disjoint-frame eligibility contract; overlapping/dependent layers retain compatibility rendering.
- 🟢 [Done] Apply the plan to worker-result installation and supported committed integer translations/Undo/Redo. New/resized/missing composition targets still receive a full clear/copy. A missing independent GPU cache also requires full initialization.
- 🟢 [Done] Clear local rectangles by copying from a GPU zero buffer, with 256-byte row alignment and chunking. The 4 MiB buffer is allocated lazily per renderer and retained for subsequent updates. No CPU zero image, pixel upload or GPU readback is required. This scratch allocation is separate from the 64 MiB object-cache retention accounting; a global GPU memory budget remains unfinished.
- 🟢 [Done] With no dirty rectangles or changed frames, return before allocating the scratch buffer, creating an encoder or submitting GPU commands.

The real GPU regression exercises the production update wrapper: move one frame by 32 pixels, remove it, restore it, leave the second frame untouched, and compare against full-layer reference pixels at 50%, 100% and 170%. It also checks the unchanged/no-op path allocates no scratch buffer. A deliberately smaller 32 KiB test zero buffer exercises row chunking. All image comparisons are exact.

For this **960 × 640, two-frame GPU regression**, the prior full-clear path cleared 2,457,600 logical RGBA bytes and copied 112,320 frame bytes per update. Moving one frame now clears 112,320 bytes (old plus new rectangle, approximately **95.4% less**) and copies 56,160 bytes (**50% less**). Removing it clears 56,160 bytes and copies zero frame bytes. Every partial update records zero full clears. These are logical GPU texel-operation byte counts, not CPU upload volume, measured hardware bus transactions or GPU elapsed time. Host encoding timings are diagnostic only; application FPS/input latency still require the outstanding benchmark protocol.

Validation: 206 CPU renderer unit/integration tests, the expanded real GPU exact-comparison regression, host compilation and all-targets Clippy with warnings denied pass. Remaining priorities include dependent text composition, fractional moves, native stroke/glyph geometry, complete profiling coverage and representative large-scene Release/application measurements.

## Follow-up: source identity and shared core profiling

- 🟢 [Done] Share thread-local diagnostics between core and renderer. Instrument committed/preview vector moves, text edits, horizontal/vertical reflow and nonempty scene journal updates. Timings remain inclusive host durations; they are not GPU timing and must not be added together. Other core mutation routes remain to be instrumented. The core text-preview example prints snapshots when metrics are enabled.
- 🟢 [Done] Validate retained composed text images with the runtime journal instance, layer source pointer/length and geometry/transform/style/visibility/structure generations. Supported integer text translations no longer clone the entire layer SVG into the composed-image cache. Source tokens are runtime-only and never dereferenced. Prepared source ownership still moves into the object cache; small lookup allocations and other SVG copies remain. Full compatibility-image installs clear the token.
- 🟢 [Done] Regression checks accept a composed image with no retained source string and reject stale move/Undo/Redo state and a foreign document clone. Existing real-GPU local composition tests retain exact full-render comparisons at 50%, 100% and 170%.

Validation: core and renderer tests passed (485 unit/integration tests; 41 manual/GPU tests ignored in the standard run). The opt-in core example emitted text-edit and journal counters. This does not establish application latency or FPS improvements. Full GPU transfer instrumentation, dependent text composition, native geometry coverage and Release application benchmarks remain unfinished.

## Follow-up 2026-10-08: translucent independent text frames

- 🟢 [Done] Extend cropped display preparation to disjoint, contained text frames with layer opacity below 1. Keep the unmodified glyph raster in the CPU frame cache and apply the existing encoded premultiplied RGBA rounding to cropped payloads. Opacity changes avoid glyph rasterization and document-sized CPU allocation/composition. They still copy/attenuate the cropped CPU pixels and upload affected frames; this is not GPU opacity processing.
- 🟢 [Done] Include opacity in prepared-result validation, per-frame GPU reuse and partial-composition eligibility. Changed opacity forces GPU target reinitialization so stale opacity pixels cannot survive. The composed image and retained object cache carry the actual opacity. Compatibility rendering remains for overlap, dependent groups, clips, effects and unsupported sources.
- 🟢 [Done] Compare cropped preparation against the full renderer at opacity 1, 0.5, 0.01, 0, 0.75 and restoration to 1 with English/Japanese/Simplified Chinese text. Assert that all changes reuse the two original glyph rasters. Extend the real GPU movement/removal/restoration regression to opacity 1, 0.5, 0.01 and 0 at 50%, 100% and 170% zoom.

This removes the previous opacity-1-only restriction for preparation, not all translucent drag-preview fallback restrictions. Overlapping text GPU blending and effects remain unfinished. No application speedup/FPS claim is inferred from these correctness tests.

Validation for the 2026-10-08 follow-up: 207 renderer unit/integration tests pass, plus the explicit real-GPU opacity/zoom comparison. All-targets Clippy with warnings denied, macOS host cargo check, and formatting/whitespace checks pass.

## Follow-up 2026-10-08: overlapping-text compatibility work

- 🟢 [Done] In full text-frame compatibility composition, skip all-zero source pixels and directly copy opaque source pixels. Other pixels use the unchanged integer source-over formula. Exhaustively compare every source/destination alpha combination against that formula. The all-zero check deliberately includes RGB, preserving behavior for non-premultiplied anomalous zero-alpha input too.
- 🟢 [Done] Apply layer opacity only to covered row intervals. Merge touching/overlapping intervals before correction so a pixel is corrected exactly once after all frames are composed. Uncovered canvas pixels remain zero. Glyph rasters retain their original values. Add `text_covered_opacity_bytes` and inclusive host timing `text_covered_opacity` diagnostics.
- 🟢 [Done] Compare overlapping colored text against full rendering for opacity 1, 0.5, 0.01, 0 and 0.75; test interval sorting, merging, uncovered rows and exact rounding separately.

The CPU-only manual test uses a 2048² buffer with two overlapping intervals across 80 rows. Opacity correction visits 230,400 bytes instead of 16,777,216 (about 98.6% fewer logical bytes). After one warm-up, 20 test-profile samples measured median correction times 1,823 µs for full traversal and 38 µs for interval traversal. Allocation/cloning and text rasterization are outside the timed region, the full algorithm always ran first, and the input is synthetic. These measurements are scoped diagnostics, not unbiased end-to-end application speedups or GPU timing. Reproduce with `cargo test -p lumapaint-renderer --features skia covered_opacity_timing -- --ignored --nocapture`.

Validation: 209 normal renderer unit/integration tests, the expanded overlap/opacity regression and manual CPU comparison pass; all-targets Clippy with warnings denied, macOS host check, formatting and whitespace checks pass. The full CPU canvas allocation/composition remains in this compatibility route. Exact overlapping GPU composition, local target updates for dependent frames and global memory/latency benchmarks remain unfinished.

## Follow-up: cropped CPU composition for overlapping text

- 🟢 [Done] For canonical, contained text layers with at most 64 objects and normal independent structure, collect original cropped glyph rasters in stacking order. When their padded bounds overlap, compose them with the same integer encoded source-over formula in a single union-bounds CPU image. Apply layer opacity after composition. Upload the resulting cropped image and copy it into the existing full-sized GPU display target. No document-sized CPU composition image is allocated by this display path unless the union bounds themselves span the document.
- 🟢 [Done] Keep the per-object glyph raster cache across edits and Undo/Redo. The overlapping composite is rebuilt on each preparation; unchanged individual glyphs avoid rasterization but still participate in cropped CPU composition. The transient union image can approach the document size and is not a new persistent cache.
- 🟢 [Done] Mark this result as non-independent and discard independent object GPU retention for it. Selection, drag previews and committed moves continue through existing compatibility paths. Never treat the combined image as one selectable original object. A composed layer still uses a full-sized GPU target and full GPU clear/copy for this subset. Dependent groups, masks/effects, outside-canvas content and unsupported sources retain compatibility preparation.
- 🟢 [Done] Compare overlapping colored text at opacity 1, 0.5, 0.01, 0 and 0.75 against the full renderer. Add an edited-text Undo/Redo regression that reuses all original glyph rasters. Extend the real GPU regression to overlapping text at opacity 1, 0.5, 0.01 and 0, with exact presentation at 50%, 100% and 170%. GPU movement/removal in this harness exercises the combined image-copy primitive, not individual-object application movement.

Validation: the prior 209-test normal renderer suite and the new edit/Undo/Redo regression pass (210 normal tests in total); explicit real-GPU overlapping/nonoverlapping comparisons, macOS host compilation, and all-targets Clippy with warnings denied pass. Overlapping composition remains on the CPU in a cropped region; shader-based exact blending and local dependent-frame updates remain unfinished. No FPS or end-to-end latency improvement is claimed.

For the 960 × 640 overlapping-text fixture, CPU composite payload is 17,500 bytes versus a 2,457,600-byte document canvas (about 99.3% smaller). This is a fixture-specific temporary-image payload comparison; total memory also includes cached glyph rasters, source metadata, upload copies and the full GPU target. Large or widely separated text bounds can substantially reduce this benefit.

## Follow-up: bounded reuse of overlapping-text composites

- 🟢 [Done] Retain up to 8 overlapping-text composites per frame-cache instance with a 16 MiB accounted payload budget. Include source and layer-ID string lengths and cropped RGBA payload in accounting. Exact layer ID, source, viewport dimensions and opacity guard hits. Least-recently-used entries are evicted; oversized results are returned without retention.
- 🟢 [Done] Return the same `Arc<CroppedFrame>` on a hit before per-frame raster-cache lookup and overlap composition. Warm preparation and retained Undo/Redo states reuse the composite itself, not just its glyph rasters. Add hit/miss/eviction/budget diagnostics and expose composite accounting separately in `FrameCacheStats`.
- 🟢 [Done] If the current full GPU layer already matches the incoming overlapping result's source identity, dimensions and opacity, installation returns before cropped upload or GPU clear/copy. Cold or changed results retain the prior upload/composition behavior.

Regression coverage checks shared Arc identity, exact full-render pixels, opacity changes/restoration, changed viewport compatibility fallback, edited text Undo/Redo, LRU entry eviction, payload accounting and oversized bypass. The new 16 MiB budget is separate from the existing 64 MiB glyph cache and object/GPU caches, per cache instance/worker. Allocator overhead, outstanding worker/consumer Arc references and GPU memory are not counted; this is not a global memory ceiling. Whole-source comparison and result metadata clones remain. Exact GPU overlap blending and partial dependent-frame updates remain unfinished.

Validation for composite reuse: 212 normal renderer unit/integration tests pass (41 GPU/manual tests ignored in this standard run); all-targets Clippy with warnings denied, macOS host cargo check, formatting and whitespace checks pass. Pixel-generation/upload shaders were unchanged in this follow-up; this run adds CPU cache/identity regressions rather than a new GPU timing or application latency measurement.

## Follow-up: local CPU updates inside overlapping composites

- 🟢 [Done] Retain ordered object-ID/source/bounds metadata with each overlap composite. Include retained metadata strings in the existing 16 MiB accounted payload budget.
- 🟢 [Done] For a prior composite with matching layer, viewport and opacity, use local rebuilding when object count/order and overall cropped bounds remain stable. Form a conservative dirty bounding rectangle from old/new bounds of changed frames, clear it, and replay every intersecting current frame in stacking order. Apply opacity only to the rebuilt rectangle. This preserves the encoded integer source-over and layer-opacity rounding.
- 🟢 [Done] Copy the immutable retained cropped image before updating, preserving earlier worker results and Undo/Redo entries. Whole-crop copying remains; the optimization reduces clear/composite/opacity work, not all image copying. Changed overall bounds, insertion/removal/order, missing cache or changed opacity use full cropped composition. GPU installation still uploads the cropped result and clears/copies the full GPU layer for changed overlapping results.
- 🟢 [Done] Add host timing and counts for local updates, copied bytes and dirty bytes. Validate a moved/recolored middle frame under an overlapping foreground at opacity 0, 0.01, 0.5 and 1 against fresh full cropped composition. Assert that prior pixels are unchanged; changed size/order/count correctly decline local rebuilding.

Validation: 212 existing normal renderer tests plus the new focused regression pass (213 tests total); all-targets Clippy with warnings denied, macOS host check, formatting and whitespace checks pass. No GPU shader/copy primitive changed in this follow-up. Partial GPU uploads for overlapping results, structurally changing local updates, tiled immutable storage and end-to-end Release latency measurements remain unfinished.

## Follow-up: conditional GPU patches for overlapping text

- 🟢 [Done] Attach the exact previous composite source and dirty bounds to a locally rebuilt result. Overlap cache hits return no patch, and structural/bounds/opacity changes return full cropped results.
- 🟢 [Done] Retain the overlap composition source in the GPU layer cache for exact base matching. Patch only an existing retained-text target with matching base source, opacity and document dimensions and no GPU effects. Current result source/dimensions/opacity and layer effects are validated before installation. Missing/mismatched base falls back to full initialization, preventing out-of-order worker results from patching an unrelated image. This adds a retained source string for overlapping images; independent integer-move source-copy optimization is unchanged.
- 🟢 [Done] Extract and upload only dirty RGBA rows to a temporary patch texture. Copy it into the matching full-sized GPU target without blending or whole-target clearing. Patch pixels already contain stacking and opacity; zeros erase removed coverage. Validate rectangle containment and refresh the current source identity after installation. Existing full GPU target and immutable whole-crop CPU copy remain.
- 🟢 [Done] Add patch upload/copy byte counters, host encoding timing and base-mismatch diagnostics. Test emitted base source and bounds after a real text color edit, and require full initialization for opacity changes. The real GPU comparison now uses the production patch-copy helper for overlapping movement/removal/restoration at opacity 1, 0.5, 0.01 and 0 with 50%, 100% and 170% presentation. Compare exactly against full pixels, and check patch logical bytes and zero full clears when metrics are enabled. This tests the GPU primitive; complete application/worker-latency measurement remains outstanding.

CPU validation: 213 existing normal renderer tests plus the new patch-metadata regression pass (214 tests total). Native overlap shaders, structural local updates, immutable tiled storage, global memory budgeting and Release application benchmarks remain unfinished.

Patch follow-up validation also passes the explicit real-GPU test with `LUMAPAINT_RENDER_METRICS=1`, all-targets Clippy with warnings denied, final macOS host check, formatting and whitespace checks. Logical patch bytes and no-full-clear assertions pass; GPU elapsed time/FPS are not measured.

## Follow-up: shared CPU text retention budget

- 🟢 [Done] Limit glyph and overlapping-composite retention together to 64 MiB per `FrameRasterCache` instance. Keep composite-specific limits of 16 MiB/8 entries and the 512-entry glyph limit. Combined retained payload can no longer reach the previous separate-budget sum of 80 MiB. This is a capacity change, not a measurement of actual peak memory savings.
- 🟢 [Done] When the shared budget is exhausted, evict the oldest access across both categories; category-specific entry/sub-budget limits still apply. Outstanding Arc users remain immutable and usable after eviction. Oversized glyph/composite results retain the previous nonretaining fallback. Add shared-budget eviction diagnostics and expose combined retained bytes/budget in `FrameCacheStats`.
- 🟢 [Done] Include layer/object ID string payload in glyph accounting, in addition to source and pixels. Composite metadata accounting stays in place. Update the existing accounting regression accordingly and add mixed-category eviction/accounting tests with either category oldest and external image references still alive.

This is one CPU text-cache-instance budget. The parsed-text cache, renderer object cache, GPU targets/scratch textures, allocator overhead, transient clones and references held by workers/consumers remain outside it; multiple instances have separate ceilings. A renderer/application-wide CPU/GPU memory coordinator and tiled immutable composite storage remain unfinished.

Shared-budget validation: 215 normal renderer unit/integration tests pass (41 GPU/manual tests ignored), all-targets Clippy with warnings denied, macOS host check, formatting and whitespace checks pass. Rendering/pixel algorithms and GPU transfer primitives did not change in this follow-up.

## Follow-up: recycle exclusively owned overlap buffers under pressure

- 🟢 [Done] When composite entry/estimated payload/shared-budget pressure requires room, allow the selected prior overlap entry to leave the cache and transfer its exclusively owned image into local rebuilding. Reuse its pixel allocation instead of copying the entire cropped image. Other stable-bounds conditions and the exact GPU patch base-source contract remain unchanged.
- 🟢 [Done] Use Arc copy-on-write as the final ownership guard. Shared worker/consumer images remain immutable and are copied; when there is spare retention capacity, preserve the prior cached image for warm Undo/Redo. No document history is removed. Under pressure, eviction can cause a later Undo state to need recomposition, as with other cache eviction.
- 🟢 [Done] Declined local updates restore the removed entry and its accounting before full composition. Only successful recycling counts the previous cache version as evicted; reinsertion uses the existing per-category/shared budget checks. Add recycled-update/entry counters; copied-byte diagnostics report only actual full-image copy-on-write work.
- 🟢 [Done] Tests verify allocation address reuse for an exclusive buffer, exact pixels against fresh composition, unchanged external shared pixels, and integrated recycling at the 8-entry limit after a real text-color edit. Verify emitted GPU patch metadata and retained-byte accounting after replacement.

Recycling is conditional, not universal: shared images and ordinary multi-version cache retention still require whole-crop copying. It avoids the large pixel allocation/copy in the eligible case, not all metadata/Arc allocation. Persistent tiled storage, structural local updates and application-wide CPU/GPU memory coordination remain unfinished. No measured FPS or end-to-end speedup is claimed.

Recycling validation: 217 normal renderer unit/integration tests pass (41 GPU/manual tests ignored); all-targets Clippy with warnings denied, macOS host check, formatting and whitespace checks pass. GPU copy/shader primitives are unchanged in this follow-up.

## Follow-up: borrowed-row direct GPU patch upload

- 🟢 [Done] Replace packed patch-vector creation, temporary patch texture/bind group allocation and texture-copy command submission with `Queue::write_texture` into the retained GPU target. Borrow the existing composite's row span with its original stride; only the requested rectangle is written. The exact base-source/opacity/size and current-generation validation stays unchanged.
- 🟢 [Done] Validate nonempty rectangles, row stride, buffer length, containment, u32 conversion and GPU target bounds before writing. Borrowed spans include row gaps; those pixels are not uploaded as destination coverage. Count logical RGBA patch bytes and borrowed span bytes separately, with inclusive write host timing. No explicit submit is needed here: the write is ordered with the renderer's subsequent queue submission.
- 🟢 [Done] Unit-test slice pointer identity, unaligned stride, exact final-row length, single-row input and invalid bounds/layout. Update the real GPU regression to call the production direct-write helper, with extra source columns exercising ignored pixels and non-256-aligned row strides. Compare exact pixels for overlap movement/removal/restoration, opacity 1/0.5/0.01/0 and zoom 50%/100%/170%; assert logical upload bytes and no full clear.

This removes application-owned patch packing and per-patch GPU resources, not wgpu's internal staging copy. Shared whole-crop CPU copy-on-write still remains; eliminating it requires tiled/chunked immutable composite storage and compatibility materialization. Current GPU target and cached SVG metadata remain unchanged in size. GPU elapsed time and end-to-end speedup remain unmeasured.

Borrowed-row upload validation: 217 existing normal tests plus the new layout regression pass (218 normal renderer unit/integration tests total), as does the explicit real-GPU test with metrics enabled. All-targets Clippy with warnings denied, macOS host check, formatting and whitespace checks pass.

## Follow-up: immutable tiled RGBA foundation (not yet in the text cache)

- 🟢 [Done] Add Rust `tiled_rgba::TiledRgba` for immutable display snapshots with 128×128 tiles. Snapshot clones share tile Arc payloads; replacing a region copies only intersecting changed tiles. Identical replacements keep all pixel allocations shared. Preserve encoded RGBA bytes and replace transparent coverage without blending. This is separate from the editable pixel-document tile/history contract.
- 🟢 [Done] Validate source dimensions, region containment/overflow and payload length before updates. Borrow tile row spans for GPU upload, validate target usage/format/dimensions before the first write, and provide contiguous materialization only for APIs requiring it. Snapshot metadata is still cloned and tile metadata traversed; this is not zero-cost constant-time scene editing.
- 🟢 [Done] Test tile-boundary edits, cropped edge tiles, prior snapshot immutability, identical-update sharing, invalid-input atomicity and row-stride reconstruction. A 2048² one-pixel edit copies one 65,536-byte tile instead of the whole 16,777,216-byte image (99.6% fewer pixel-copy bytes in this isolated test). Initial conversion copies pixels into tiles, and this is not an application speedup measurement.
- 🟢 [Done] Extend the real GPU exact comparison to tiled borrowed-row upload alongside the existing direct patch path, at opacity 1/0.5/0.01/0 and zoom 50%/100%/170%, including movement/removal/restoration.
- 🟢 [Done] Connect tiled snapshots to large overlapping text composite retention/local rebuilding and GPU installation, including budget accounting, no-op/Undo/Redo and fallback materialization (integration follow-up below). The foundation by itself did not change application copy behavior; smaller composites still use contiguous cropped images.

Logical snapshot payload accounting counts all tiles even when shared. Physical unique allocation accounting, global GPU/CPU budgets and application Release latency remain unfinished.

Tiled-foundation validation: 221 normal renderer unit/integration tests pass (41 GPU/manual tests ignored in the normal run); explicit real-GPU direct/tiled comparison, all-targets Clippy with warnings denied, macOS host check, formatting and whitespace checks pass.

## Follow-up: integrate tiles into retained overlapping text

- 🟢 [Done] Use tiled snapshots for contained canonical overlapping text composites with union payload at least 256 KiB. Smaller composites retain the contiguous/local-recycling path. Cache hits return the retained tiled Arc before glyph preparation. Retain immutable tiles and exact ordered source/bounds metadata under the existing 16 MiB composite and shared 64 MiB text budgets. Accounting conservatively charges each snapshot's logical tile payload even where tiles are shared, not physical unique allocation.
- 🟢 [Done] For stable bounds/order/count/opacity, compose just the conservative dirty rectangle in encoded stacking order, apply opacity once, and patch the snapshot. Only touched changed tiles copy; old results and cached Undo/Redo states keep their pixels. Changed structure/bounds or opacity perform fresh cropped composition and conversion. Initial conversion still allocates/copies the cropped image into tiles. Tile metadata cloning and the dirty temporary rectangle remain.
- 🟢 [Done] When the renderer retains the exact GPU base source/dimensions/opacity without effects, upload borrowed tile rows directly to the global dirty rectangle. A cold target or mismatched base materializes a correct contiguous cropped image and follows the existing full GPU initialization path. No heavy payload crosses JavaScript IPC. The full-sized GPU layer target remains.
- 🟢 [Done] Integrated test uses English/Japanese/Simplified Chinese text with a large frame and a small overlapping frame. Verify exact full-render pixels, original shared-image immutability, retained Arc reuse after Undo/Redo, opacity/bounds-change fallback, and payload accounting. Explicit GPU regression also checks nonzero source-column offsets with direct/tiled upload, transparency, movement/removal/restoration and three zooms. It validates upload primitives, not complete application input latency.

The integrated 2048² fixture has a 2,185,508-byte cropped composite. A small text color edit copies 131,072 tile bytes (about 94% fewer than copying that whole composite), allocates a 24,832-byte dirty rectangle, and rasterizes one changed 26,656-byte glyph region while reusing the other. Diagnostics assert no contiguous tiled-image materialization during preparation of this local edit. These are scoped logical bytes, excluding metadata, queue staging and GPU memory, not a total-memory/FPS comparison.

Remaining work includes local structural edits and changing outer bounds, eliminating dirty-region temporaries, accounting for physical shared-tile allocations/global renderer budgets, retained native glyph rendering and Release application latency benchmarks.

Tile integration validation: 222 normal renderer unit/integration tests pass (41 manual/GPU tests ignored in the normal run), the integrated cache regression passes with metrics enabled, and the explicit real-GPU direct/tiled comparison passes. All-targets Clippy with warnings denied, macOS host check, formatting and whitespace checks pass.

### Structural overlap updates (2026-10-08)

- 🟢 [Done] Share an exact-ID dirty-region planner between contiguous cropped composites and tiled composites. Insertions and deletions include the new or old coverage; source/rectangle changes include both. Reordering compares the relative ranks of surviving IDs and conservatively includes every displaced survivor. Insertion alone does not dirty survivors whose absolute indices shift.
- 🟢 [Done] Replay current intersecting frames in stacking order inside the dirty rectangle, then apply layer opacity once. Existing exact GPU base-source checks and borrowed-row uploads consume the resulting patch without changing GPU primitives. Duplicate IDs, changed overall bounds, changed opacity and unsupported preparation routes retain full fallback behavior.
- 🟢 [Done] Synthetic structural sequences insert, reorder and remove frames through both contiguous and tiled preparation. Compare every result against fresh full composition at opacity 0, 0.01, 0.5 and 1; verify original shared pixels remain unchanged, exact prior-source patch metadata, insertion/deletion coverage and duplicate-ID fallback. Existing document edit/Undo/Redo tests remain enabled. These structural sequences exercise cache preparation directly, rather than full application interaction or document-history integration.

The planner still scans all prepared text-frame keys, and dirty coverage is one conservative rectangle. Large reorders can therefore update much of the image. Changed overall cropped bounds still require full composition. This stage does not measure application FPS or input latency.

Structural-update validation: 223 normal renderer unit/integration tests pass; 41 manual/GPU tests remain ignored in the normal run. All-targets Clippy with warnings denied, macOS host compilation, formatting and whitespace checks pass. GPU primitives were unchanged and were not rerun in this stage.

### Direct tile construction (2026-10-08)

- 🟢 [Done] Build cold/fallback overlapping-text composites directly in 128×128 regions. Each region replays intersecting frames in the existing encoded source-over order and applies opacity once, then transfers ownership of its pixel vector into the retained tile. The former full-crop temporary and subsequent full-crop tile copy are removed.
- 🟢 [Done] Validate dimensions before invoking the builder and reject malformed tile payloads or rendering errors without publishing a partial snapshot. Verify a 259×131 edge-tile image against contiguous expected pixels and bound the largest generated tile to 65,536 bytes. Existing large overlapping-document tests cover initial preparation, opacity changes, changed bounds, local edits and Undo/Redo against full raster output.

This improves temporary pixel allocation for the existing ≥256KiB tiled route. It does not share unchanged tiles across changed crop origins/dimensions: those cases still recompose all tiles and initialize the GPU target. Tile metadata, retained payload, frame images, compatibility materialization and GPU memory remain separate costs. Each tile visits the prepared frame list (currently at most 64); no application latency/FPS improvement is claimed. `text_tiled_initial_tile_bytes` records total generated tile payload, not peak memory.

Direct-tile validation: 224 normal renderer unit/integration tests pass (41 manual/GPU tests ignored). All-targets Clippy with warnings denied, formatting and whitespace checks pass. GPU primitives are unchanged in this stage.
macOS host compilation also passes.

### Crop changes, direct GPU replacement and native-query follow-up (2026-10-08)

- 🟢 [Done] Rebuild changed crop dimensions/origins without a whole-crop copy. Index old tile rectangles in the new crop coordinates and share exact global coverage outside dirty bounds; rebuild boundary/alignment changes one tile at a time. Reuse matching old payloads even after a dirty tile renders identical bytes. Misaligned crops render afresh, preserving exact pixels. Budget accounting remains conservative logical payload per snapshot, including shared tiles.
- 🟢 [Done] Stable-bounds local updates now render affected tiles directly instead of building one potentially huge dirty-rectangle buffer and copying previous tile pixels. Each generated tile is at most 65,536 bytes and becomes retained payload directly. `text_tiled_dirty_allocated_bytes` now counts aggregate generated affected-tile payload, not a single packed rectangle; `text_tiled_copied_pixel_bytes` is zero for this preparation route because old tile pixels are not copied. Changed glyph raster and metadata copies are separate.
- 🟢 [Done] Replace cold/mismatched/changed-crop tiled results directly in the full GPU target. Validate crop/target before clearing; submit a transparent GPU clear before staging borrowed tile rows. Preserve the target when dimensions match. Production tiled display no longer calls `materialize_tiled`; contiguous compatibility materialization exists only in test reference conversion. Full GPU clears/uploads still occur when the exact prior source cannot support a local patch, but full CPU materialization, temporary crop textures and extra per-crop GPU geometry are eliminated.
- 🟢 [Done] Real-GPU replacement regression verifies two different edge-tile crops/origins, contraction to a transparent single pixel, erased old coverage, exact RGBA bytes and rejected invalid replacement retaining the valid result. Existing exact GPU local-transfer/multizoom tests cover the unchanged patch primitive.
- 🟢 [Done] Cache native local path bounds once during verification. Transform-only journal refits apply the matrix to those bounds without reparsing SVG or creating curve vectors. Add `native_path_parses`; opt-in transform regression asserts no parse during refresh. Initial verification/geometry creation still parse.
- 🟢 [Done] Add `SpatialIndex::query_into` with retained result/stack buffers, preserve conservative unknown/invalid-bound behavior and drawing order. Native draw lists retain capacity across frames; drag candidates use the retained object-ID position map instead of enumerating every object. Add visited-node diagnostics and pointer/capacity regression. Initial verification, layer-source synchronization and geometry validity comparisons remain broader walks.

The new Release `bench_spatial` example compares allocated and retained-buffer queries and single-entry refits at 1,000/10,000/100,000/1,000,000 entries, with fixed 512 visible candidates, warm-up and p50/p95/p99 output. It measures only the spatial index, not application capacity or FPS. Source: `crates/lumapaint-core/examples/bench_spatial.rs`; command: `cargo run --release --offline --locked -p lumapaint-core --example bench_spatial -- --samples 1000`.

The six-zoom native diagnostic compares screen-transformed cubic fill alpha against the explicit compatibility backend on a 128×128 fixture. At 25%/50%/100%/150%/200%, mean alpha differences were 0.100/0.177/0.327/0.262/0.073 out of 255; maximum edge differences were 54/50/48/54/42. The 400% viewport was entirely inside the hole and has no meaningful edge comparison. This does not pass a general quality gate: low average error hides local edge differences, and this single fill fixture covers neither text/strokes nor Adobe output. Normal-zoom native routing remains disabled.

- 🟢 [Done] Native verification now rejects unsupported appearance before generating a verification SVG or building a BVH. Cache the rejected source/journal state for unchanged frames; source-incompatible eligible layers also avoid BVH construction. The GPU regression asserts zero verification SVG/BVH/path work for a stroked unsupported layer. Transform-journal validation/refitting no longer allocates filtered change lists.

Latest spatial-index Release results (arm64, Rust 1.98.1, 10,000 samples per phase; host microseconds):

| Entries | Retained query p50 / p95 / p99 | Allocating query p50 | Exclusive refit p50 | Shared-snapshot refit p50 / p99 | Query visited nodes |
| --- | --- | --- | --- | --- | --- |
| 1,000 | 7.417 / 7.625 / 7.708 | 8.208 | 0.375 | 4.417 / 4.584 | 1,037 |
| 10,000 | 7.125 / 7.250 / 7.334 | 8.334 | 0.500 | 7.500 / 7.792 | 1,041 |
| 100,000 | 7.708 / 8.125 / 8.500 | 8.875 | 0.625 | 10.459 / 10.792 | 1,049 |
| 1,000,000 | 7.416 / 8.166 / 8.209 | 8.833 | 0.833 | 13.917 / 14.375 | 1,055 |

Queries return exactly 512 candidates. Shared-snapshot refits keep an old index alive during the timed update and verify its old bound afterward; they exercise persistent page copy-on-write rather than an exclusive index only. Initial builds and snapshot/query/drop checks are outside those refit timings. Query phases are sequential, not randomized; these results are not a controlled application before/after speedup or the required 60-second × 5 acceptance protocol. Earlier runs overlapped compilation and showed materially higher tail times. No application FPS or million-object import/edit capability follows from this microbenchmark.

Validation: 505 normal core/renderer unit/integration tests pass (43 manual/GPU tests ignored). Opt-in metrics run passes 24 text-cache tests including crop expansion/contraction, shared tiles and zero materialization. Subsequent native validation/early-rejection refinements pass the focused native tests and real-GPU fill/movement/Undo/Redo regression with six-zoom quality diagnostics. The new direct tiled-GPU replacement regression also passes. All-targets Clippy with warnings denied, the new benchmark Clippy, macOS host compilation, formatting and whitespace checks pass. Global GPU elapsed time, complete allocation/transfer/RSS totals and application input-to-present latency are still unmeasured.

### Larger retained-text layers (2026-10-08)

- 🟢 [Done] Increase the canonical contained display-text entry limit from 64 to 4,096 and the glyph-cache entry limit from 512 to 4,096. Keep the combined 64 MiB logical retention budget and composite sub-budgets unchanged. Entry count does not override byte-based eviction. No document format/capacity contract or raster-quality setting changes.
- 🟢 [Done] For more than 64 prepared frames, build an exact cropped-rectangle spatial index and query prior candidates with reusable buffers instead of comparing every pair. Preserve strict rectangle intersections (touching edges do not overlap), conservative composition order and the existing tiled encoded-blend route. Compare indexed results against the pairwise reference at both sides of the threshold and at 4,096 frames.
- 🟢 [Done] Prepare a real 1,000-object canonical text layer via cropped display, compare every output pixel against full rendering, and verify all glyph rasters reuse on the second preparation under the existing memory budget. Large dependent composites still replay the frame list per dirty tile; imported/mixed/grouped/clipped features remain compatibility paths.
- ⭕️ [Pending] Cross-platform GPU quality/driver verification outside the local macOS development environment and Adobe font/color-managed reference comparisons without matching font/color settings. The first local geometry export comparison is now available (see below).
- 🟠 [Next] The locally testable native stroke/gradient/clip implementation and quality corrections, broader Scene incrementality and application resource instrumentation remain unfinished. These are not moved to Pending merely because they are substantial work.

Larger-text validation: 227 normal renderer unit/integration tests pass (42 manual/GPU tests ignored). The expanded opt-in 1,000-text regression additionally changes one object's color: exactly one new glyph raster and 999 raster-cache reuses, zero full-canvas allocations during display preparation, and exact equality with full rendering afterward. Reference conversion intentionally allocates a full canvas outside the measured preparation interval. All-targets Clippy with warnings denied, macOS host compilation, formatting and whitespace checks pass. The count limit and overlap indexing do not constitute an end-to-end latency or native-glyph rendering result.

## Native straight stroke subset (2026-10-08)

- 🟢 [Done] A single opaque stroke-only object can use retained native GPU geometry at the existing high-zoom gate. Horizontal/vertical straight paths support centered uniform width, butt/square caps and bounded dash expansion. Skia produces the outline once; verified rectangular contours use exact pixel-area coverage in the GPU shader. Stroke width/style changes invalidate geometry, while translation reuses resident buffers without reparsing the path.
- 🟢 [Done] Real GPU comparisons cover butt/square caps, solid/dashed horizontal lines, retained integer translation and half-pixel translation. Integer fixtures match the explicit compatibility raster byte for byte; fractional fixtures differ by at most one channel unit. These fixtures do not establish general stroke quality or Adobe equivalence.
- 🟠 [Next] Curved or multi-segment paths, round caps/joins, multiple stroke objects, transparent strokes, mixed fill/stroke, rotation/shear and unsupported styles retain compatibility rendering. Dash expansion and rectangle counts are bounded. Normal-zoom native routing remains disabled pending the separate fill-quality correction.

Validation for this subset: 228 renderer CPU/integration tests pass (216 unit + 12 integration; 42 GPU/manual tests excluded from the ordinary run). The expanded real GPU regression passes explicitly. All-target renderer Clippy with warnings denied, host compilation, formatting and diff whitespace checks pass.

## Warm synchronization and compatibility pixel ownership (2026-10-08)

- 🟢 [Done] Native synchronization compares borrowed layer identities before allocating a snapshot. Idle/pan/zoom with unchanged sources now creates no snapshot Vec or cloned layer IDs. A journal cursor change with unchanged sources also reuses the snapshot; changed sources reuse Vec capacity. The GPU regression checks that both the snapshot allocation and retained layer-ID allocation stay stable across warm frames.
- 🟢 [Done] Unsupported-native reason counts are computed once during layer verification and cached. Diagnostics reuse these counts instead of walking every object in a rejected layer on every frame. An eligible but noncanonical SVG reports `fallback.native_source_mismatch`. Warm rejected-layer tests verify retained reason counts and zero SVG generation/BVH build/path parsing.
- 🟢 [Done] Cold SVG and workspace uploads now borrow the owned pixel buffer for GPU upload and then move that allocation into the comparison cache. Previously these callers cloned the complete buffer for comparison retention. A 4096-square RGBA8 image therefore avoids one 64 MiB host allocation/copy on this path. This is a code-level byte accounting result, not a measured latency improvement; wgpu staging copies, the original raster allocation and compatibility pixel comparison remain.
- 🟠 [Next] Layer-identity comparison still visits layers; ordinary Scene and compatibility rendering still have broader object/source processing. These changes do not establish a constant-cost application frame or complete the specification.

Validation: 228 normal renderer tests pass, the explicit real GPU regression passes with `LUMAPAINT_RENDER_METRICS=1`, renderer all-target Clippy with warnings denied and host compilation pass. Formatting and diff checks pass. No end-to-end before/after timing or Adobe comparison is claimed for these changes.

## Qualified straight strokes at normal zoom (2026-10-08)

- 🟢 [Done] Production routing now allows qualified single opaque straight strokes at normal zoom. Six screen zooms (25%, 50%, 100%, 150%, 200%, 400%) match the compatibility raster byte for byte for horizontal/vertical lines with butt/square caps, solid and dashed patterns. Warm zoom changes regenerate no geometry, upload no geometry, generate no verification SVG and rebuild no BVH. Existing integer/fractional drag comparisons remain.
- 🟢 [Done] Transform-only journal validation rechecks eligibility of changed objects before refitting. Rotating an eligible axis-aligned stroke now invalidates its native certification and returns to compatibility rendering. A regression verifies rejection before any spatial refit.
- 🟠 [Next] Cubic fills at normal zoom remain gated: existing diagnostics still show local edge differences. General stroke paths, mixed/transparent compositing, gradients and clipping remain unfinished. These stroke fixtures are not a general quality or performance benchmark and do not establish Adobe equivalence.

Validation: 229 normal renderer tests pass (217 unit + 12 integration), plus the explicitly run GPU regression. Renderer all-target Clippy with warnings denied, host compilation, formatting and diff checks pass. No end-to-end frame-time improvement is claimed from these correctness/resource checks.

## Nonblocking GPU display-frame timing (2026-10-08)

- 🟢 [Done] `gpu_timing.rs` adds a fixed three-slot timestamp/readback ring. With `LUMAPAINT_RENDER_METRICS=1` or `LUMAPAINT_RENDER_DEBUG=1`, request `TIMESTAMP_QUERY` only when supported. Disabled diagnostics create no query/readback resources. Unsupported devices continue without timestamps and report `gpu_timestamps_unsupported`.
- 🟢 [Done] Timestamp the beginning of the preview pass and the end of the final selection-overlay pass. Resolve two ticks per sampled frame; convert using `Queue::get_timestamp_period`. Later frames collect completed mappings via nonblocking `PollType::Poll`. No production wait, per-frame readback allocation or unbounded pending queue. All three busy slots cause a skipped sample, counted as `gpu_timestamp_busy_frames`. Failed mappings are counted and their slots released.
- 🟢 [Done] Report `gpu_frame_nanoseconds` and `gpu_frame_samples` in the existing thread-local renderer profile. Divide their sum/count to obtain the mean for the completed samples reported together. Results arrive on a later frame; these counters must not be paired with that later frame's CPU timing as if they measured the same frame. This does not yet provide per-frame IDs or p95/p99 aggregation.
- 🟢 [Done] Explicit real-GPU regression fills all three slots, asserts a fourth reservation is refused, resolves/maps each slot, verifies positive elapsed times and drains/reuses the same buffers for a second batch. Waiting is restricted to the test. Production contains no synchronous GPU readback.
- 🟠 [Next] GPU timings for separately submitted image effects/paint preparation and transfers, presentation wait, memory totals, per-frame correlation/percentiles and input-to-present latency remain unfinished. Display timestamps do not include these stages. There is no before/after speedup claim for this measurement-only change.

Validation: 229 ordinary renderer unit/integration tests pass; 43 GPU/manual tests are excluded from that ordinary run. The new GPU timestamp regression passes explicitly. Renderer all-target Clippy with warnings denied, macOS host compilation, formatting and diff checks pass.

## Correlated GPU/host frames and percentile windows (2026-10-08)

- 🟢 [Done] Assign each timestamp attempt a monotonic frame ID, including attempts skipped because all slots are busy. A per-renderer stream ID distinguishes multiple rendering windows. The retained readback slot carries the original frame ID and that frame's host render duration through presentation, so a late GPU result cannot be mistaken for the current frame's CPU duration.
- 🟢 [Done] Emit `lumapaint-gpu-frame stream=… id=… gpu_ns=… host_ns=…` for completed samples. Busy attempts and invalid/failed readbacks have explicit status records with the same stream/frame identifiers. Existing aggregate GPU counters remain available.
- 🟢 [Done] Retain the last 512 completed paired samples in a fixed array. Compute nearest-rank p50/p95/p99 and 60Hz/120Hz budget exceedance counts separately for host and GPU durations. Deadline thresholds use nanoseconds: greater than 16,666,666 ns or 8,333,333 ns. `lumapaint-gpu-window` reports the window sample count and lifetime attempts/completed/busy/failed totals. Divide each exceedance count by the window sample count for that measured component's exceedance rate; this is not an end-to-end display deadline miss rate.
- 🟢 [Done] Report the first completed batch, then at least every 60 additional completions. Unchanged results are not repeatedly sorted/reported. Percentile sorting uses fixed stack storage; history and pending readbacks do not grow. Unit regressions cover empty/short windows, percentile ranks, exact deadline boundaries and eviction. Real GPU regression verifies monotonic IDs across skipped attempts, correct host pairing, positive GPU intervals, drained samples and slot reuse.
- 🟠 [Next] This is a rolling window over completed samples, not the requested warmed 60-second × 5 benchmark. Results can arrive out of order; identifiers preserve correspondence while the window follows completion order. Busy/readback failures can bias sampled performance, so coverage totals must accompany reported percentiles. Host time includes diagnostic overhead and presentation API time; GPU time covers display passes only. Input-to-present latency, separate GPU job/transfer timings, complete memory accounting and controlled application benchmarks remain unfinished.

Validation for frame correlation/percentiles: 231 ordinary renderer tests pass (219 unit + 12 integration; 43 GPU/manual tests excluded). Independent verification imports the exact production `gpu_timing.rs` and real core metrics API: both CPU regressions and the explicit real-GPU regression pass, including reversed submission order and distinct host durations per frame. Shared-workspace renderer all-target Clippy with warnings denied and macOS host compilation also pass after transient unrelated screentone changes were completed. Diff checks pass. No claim of full benchmark completion or application FPS follows.

## Scoped GPU resource/write accounting and host RSS (2026-10-08)

- 🟢 [Done] `gpu_metrics.rs` instruments the renderer's explicit wgpu `create_buffer`, `create_buffer_init`, `create_texture`, `create_texture_with_data`, `write_buffer` and `write_texture` entry points across native vectors, text/tiles, overlays, gradients, clone stamp, retouch, effects and GPU timing. Creation and write calls report inclusive host durations. Diagnostics OFF directly execute the original API operation after one enabled check. Match-bound arguments preserve temporary input lifetimes. A source coverage regression rejects unaccounted direct calls in production renderer Rust sources; test-only fixtures and the accounting module are excluded.
- 🟢 [Done] Buffer capacity counters use returned buffer sizes, including API alignment; initial data and queue writes are counted separately. Texture logical bytes include mip levels, array layers versus shrinking 3D volumes, sample counts and compressed-block edge rounding. Unknown format/storage sizes and overflow are recorded as unmeasured rather than assumed; invalid/unbounded mip counts cannot cause a diagnostic loop. These counters measure creations since the thread snapshot was drained, not live residency or GPU-driver VRAM.
- 🟢 [Done] `gpu_texture_write_source_bytes` measures the supplied slice span (which may include padding/extra rows); `gpu_texture_written_texel_bytes` measures the addressed texels/blocks. `gpu_host_to_resource_payload_bytes` combines known explicit initialization/write payloads. Native Metal texture creation in the shared Skia route is counted once as an external creation with known RGBA8 logical size; importing that same texture does not count as a second allocation or host upload. Existing SVG/native counters are narrower views and must not be added again to these totals.
- 🟢 [Done] `process_memory.rs` reads macOS `MACH_TASK_BASIC_INFO` using the installed SDK ABI and reports current/peak process RSS in bytes. Linux parses `/proc/self/status` with validated kB units/overflow checks and an optional peak. Each enabled renderer samples at most once per second and times the OS query; unavailable reads are counted. The RSS covers the Rust host process, not all WebView subprocesses or GPU allocations.
- 🟢 [Done] Unit checks cover mip/array/volume/MSAA/compressed texture accounting, unknown storage, overflow/unbounded mip input, Linux status parsing and actual macOS RSS/rate limiting. Opt-in real GPU regression verifies 8,960 supplied bytes versus 7,196 addressed texel bytes, separate creation/initialization/write counts, buffer capacity and exact readback pixels.

Key counters in `lumapaint-render-profile`: `gpu_buffer_creations`, `gpu_buffer_created_capacity_bytes`, `gpu_buffer_initial_data_bytes`, `gpu_buffer_write_calls`, `gpu_buffer_written_bytes`, `gpu_texture_creations`, `gpu_external_texture_creations`, `gpu_texture_created_logical_bytes`, `gpu_texture_initial_data_bytes`, `gpu_texture_write_calls`, `gpu_texture_write_source_bytes`, `gpu_texture_written_texel_bytes`, `gpu_host_to_resource_payload_bytes`. `lumapaint-process-memory` reports RSS gauges rather than cumulative allocation counters.

- 🟠 [Next] Live resource lifetime/deduplication accounting, CPU Vec/heap allocations, third-party Skia/driver/staging allocations, GPU-to-GPU copies and readback transfer totals, worker-thread aggregation, WebView-process RSS, Windows RSS implementation and input-to-present latency remain unfinished. These measurements establish neither whole-application memory budgets nor a rendering speedup.
- ⭕️ [Pending] Actual Linux RSS and other OS/GPU validation require the respective runtime environments; only Linux parsing is tested locally. Local macOS process sampling is verified.

Validation for resource/RSS accounting: 237 normal renderer tests pass (225 unit + 12 integration; 46 GPU/manual tests excluded from that run). The opt-in real-GPU padding/byte-accounting/readback regression passes explicitly. Actual macOS RSS acquisition, rate limiting and source coverage checks pass. Renderer all-target Clippy with warnings denied, macOS host compilation, formatting and diff checks pass.

### Retained composition texture ownership (2026-10-08)

- 🟢 [Done] SVG/text/workspace `CachedSvg` entries carry diagnostic leases. A process-wide registry deduplicates shared wgpu texture handles, measures all mip levels/samples, and reports live cache-owned texture count, logical bytes and unmeasured formats alongside the once-per-second RSS sample. Final lease release removes the registry key immediately, including release on another thread; it does not retain released textures until the next sample. Acquisition/release use keyed lookups, with no scan of the whole cache on edits. Diagnostics OFF allocate no lease or registry entry.
- 🟢 [Done] The GPU resource regression checks shared handles, replacement overlap, partial/final release, cross-thread release and an empty registry after clearing, in addition to the existing padded-upload pixel comparison.
- 🟠 [Next] This gauge covers retained composition textures, not all renderer buffers/textures. GPU submissions, bind groups outside these caches, Skia, driver/staging storage and actual VRAM release can outlive host cache ownership. Whole-renderer lifetime coverage, CPU heap accounting and the remaining phase 1 measurements are unfinished.

### CPU composite tile capacity (2026-10-08)

- 🟢 [Done] Immutable `TiledRgba` pixel storage reports process-wide live Vec capacity/allocation count and cumulative allocated/released capacity in opt-in diagnostics alongside the RSS sample. Shared Arc tiles count once; copy-on-write counts the newly allocated Vec's actual capacity, which can differ from its source capacity. Final release subtracts capacity even on a worker thread. Pixel mutation exposes slices only, preventing untracked Vec growth. Diagnostics OFF attach no ledger and perform no atomic accounting.
- 🟢 [Done] Regression coverage checks sharing, copy-on-write isolation, spare capacity, cross-thread final release, balanced cumulative capacities and untracked storage. Existing crop/rebuild/patch pixel tests cover retained output behavior.
- 🟠 [Next] These counters cover retained composite pixel Vecs only, excluding Vec/Arc headers, allocator overhead, tile metadata, fonts, documents, other image buffers and temporary allocations made before tile storage takes ownership. Atomic fields are independently sampled, not a transactional snapshot during worker activity. Whole-application CPU heap allocation/copy accounting remains unfinished; no speedup is claimed from instrumentation alone.

### Published worker aggregation and Windows working set (2026-10-08)

- 🟢 [Done] Renderer frame and macOS SVG worker snapshots publish into process-wide cumulative host counters/timings at existing batch boundaries. Individual `count`/timer calls stay thread-local. Saturating merging preserves metric names; empty repeated drains cannot double-count worker work. The once-per-second memory log includes the aggregate. Published CPU durations include nested and parallel work and must not be interpreted as wall-clock frame time. Pending thread-local work and other workers that do not publish remain outside coverage; batches are not falsely assigned to the current display frame.
- 🟢 [Done] Windows host memory acquisition uses Kernel32 `K32GetProcessMemoryInfo` with the SDK-compatible `PROCESS_MEMORY_COUNTERS` layout, current-process pseudo-handle and checked API result. Current/peak working set is reported, distinct from commit charge. The existing real-process sampler regression now runs on Windows as well as macOS/Linux. Reference: https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo
- ⭕️ [Pending] Actual Windows runtime/link and Linux RSS validation require those OS environments; this macOS environment has no Windows Rust target installed. These checks are not reported as passed.
- 🟠 [Next] Remaining phases in the status table stay unfinished. Worker publication coverage for other jobs/mutations, frame association, all CPU/GPU allocation scopes and input-to-present measurement still need implementation and validation.

### BVH refit early termination (2026-10-08)

- 🟢 [Done] Refit returns before any node mutation for identical leaf bounds. Otherwise it updates the leaf and stops ancestor propagation at the first unchanged child union: all higher ancestors depend on that unchanged union. Diagnostic counters expose parent visits and early stops. This reduces unnecessary work without changing spatial culling quality.
- 🟢 [Done] Regressions cover contained movement, unchanged movement, expansion beyond the old root and preservation of the previously published index. Existing linear-scan/refit ordering and 100,000-entry snapshot tests remain acceptance coverage. No application FPS improvement is asserted without the pending controlled benchmark.

### Normal-zoom opaque rectangular fills (2026-10-08)

- 🟢 [Done] Single canonical opaque rectangular fill objects with axis-aligned transforms and pixel-aligned screen boundaries use the existing exact GPU rectangle coverage path at normal zoom. Rectangle classification reuses parsed curve segments and is retained with GPU geometry; a bounded one-entry, at-most-4096-byte path probe prevents repeated classification of an unsupported straight polygon. Curves/arcs keep their pre-parse normal-zoom rejection. Unsupported appearance, mixed/multiple objects, fractional screen boundaries and general affine transforms retain compatibility routing.
- 🟢 [Done] Real-GPU comparisons cover both contour orientations at 25%, 50%, 100%, 150%, 200% and 400%, integer drag and return. Warm drag asserts zero native path parses, geometry rebuilds and verification SVG generation. CPU classification regressions reject trapezoids, self-crossing paths, curved sides and compound contours. Existing cubic and stroke GPU regressions remain intact.
- 🟠 [Next] Fractional fill comparison produced a maximum one-byte difference in 437 channels for the test fixture; the normal-zoom gate therefore rejects that case rather than weakening the exact comparison. Fixing general edge coverage, native gradients/clips/strokes and universal retained rendering remains unfinished. This change removes compatibility raster preparation for the qualified subset; it does not establish application FPS or completion of the full specification.

### Rust heap, GPU transfers and compute-job diagnostics (2026-10-08)

- 🟢 [Done] The opt-in Cargo `heap-profile` feature installs a System-delegating global allocator for Rust allocations from startup, across document/font/image/render/worker code. It records live requested layout bytes, peak, live allocations, allocation/reallocation/failure calls and cumulative growth/release. Failed reallocations retain the previous allocation; successful growth/shrink keeps one live allocation. Allocator callbacks use only atomics, with no locks, logging, clock or environment access. Default builds do not compile the allocator wrapper. Host/core/renderer expose the feature together; opt-in render metrics print a heap snapshot at the existing RSS sampling boundary. Fields are independently sampled and cumulative realloc growth is not a measurement of actual memcpy volume. C/C++ heap, allocator metadata/reservation, WebView subprocesses and VRAM are outside this counter scope.
- 🟢 [Done] Renderer explicit GPU buffer-to-buffer, buffer-to-texture and texture-to-texture copies plus mapped read views use common accounted entry points. The GPU fixture also exercises texture-to-buffer readback. Counters distinguish logical texel/buffer payload from padded mapped spans; they count encoded operations/views, not physical bus transactions, submission success or repeated CPU reads from the same view. A production-source regression rejects unaccounted copy/read-view APIs. The padded GPU fixture verifies 7,264 encoded copy bytes and an 8,960-byte mapped span, with unchanged logical pixels, in addition to existing creation/upload/lifetime checks.
- 🟢 [Done] Metal Skia reports active context count, cache resources/bytes, purgeable bytes and configured cache budgets per rendering thread at most once per second while rasterization is active. Reentrant borrowing returns unavailable instead of panicking. Actual Metal tests verify populated cache usage and zero observed context/cache bytes after disposing the context, preserving premultiplied pixels and Japanese text output. Skia cache bytes exclude uncached/external resources and driver memory; per-thread observations are not a whole-process physical-memory total.
- 🟢 [Done] Layer-effects batch, direct display effects, pixel gradients, clone stamp and retouch compute passes now use optional timestamp queries, with independent job kind/stream/ID, host command-encoding duration and paired GPU compute duration. Fixed three-slot readbacks and 512-completion percentile histories are reused. Only diagnostic device creation requests timestamp support; unsupported adapters retain ordinary processing. Job collection polls without adding a completion wait: readback jobs retain their existing pixel-contract waits, and direct display jobs are collected during display frames. Early return releases an unsubmitted slot and records cancellation; pending mappings are never prematurely reused. Busy/failure/cancellation coverage remains distinct. GPU compute time excludes copies, upload, Skia-private command buffers and presentation.

Validation: heap allocation/zeroing/growth/shrink/failure and worker tests; real GPU copy/readback accounting; real Metal cache/clear/pixel/text checks; compute timestamp cancellation/correlation; effect, gradient and retouch CPU-reference comparisons. These instrumentation changes do not claim an application speedup or completion of phases 2–8.

For a diagnostic host build, use `cargo build --offline --locked -p lumapaint --features heap-profile`. Enable `LUMAPAINT_RENDER_METRICS=1` when launching that build. Keep ordinary builds without `heap-profile` as the performance baseline, since allocator atomics and logging introduce diagnostic overhead. All counters remain scoped as described above; they must not be summed with narrow path-specific counters a second time.

### Illustrator 2026 output comparison (2026-10-08)

- 🟢 [Done] Added `adobe_compare` to generate a self-contained 256-point SVG, render the actual compatibility backend, and compare premultiplied RGBA without resizing or accepting an arbitrary tolerance. Dimension mismatches fail.
- 🟢 [Done] Opened the fixture in the installed Illustrator 2026 and exported actual PNG at 72 ppi with artboard bounds and Art Optimized antialiasing. Saved the SVG, Adobe PNG, output settings and caveats in `crates/lumapaint-renderer/tests/fixtures/adobe-2026/`. No user documents were edited.
- 🟢 [Done] Measured Resvg compatibility output: 5,592 / 65,536 changed pixels, maximum channel error 77, mean channel error 0.213341 (0–255 scale). Interior opaque and half-transparent fill samples match exactly. Stroke/cubic/clip edges and gradient rounding differ. Illustrator showed a clipping import warning, although the circular clip remains visible in its export.
- 🟠 [Next] Extend the comparison to native GPU output, text with confirmed matching fonts, fractional transforms, masks and effects. The measured edge differences do not establish Adobe-equivalent quality and do not justify removing compatibility fallback.

### Completed job/session publication (2026-10-08)

- 🟢 [Done] Added a thread-bound, nested `performance::Batch` guard. Only the outermost completed batch publishes to process totals, and its inclusive host timer is finalized before publication. Empty snapshots avoid locking the process totals. Diagnostics-disabled builds do not access batch thread-local state.
- 🟢 [Done] Fixed superseded SVG worker jobs losing their already-performed measurements: cancellation now publishes instead of the next iteration discarding its counters. Tile projection jobs and font prewarming also publish at job completion.
- 🟢 [Done] All 35 explicit Tauri `spawn_blocking` call sites now use the central diagnostic wrapper. A guard is created inside the worker closure, so it cannot move across `.await`. Early return, error, and unwind publish completed work before a pooled thread is reused. Font catalog and preview retain named nested batches.
- 🟢 [Done] macOS session guards publish edit, render and canvas-callback host work, including text moves and Undo/Redo through their shared session boundary. Nested sessions do not independently lock process totals.
- 🟢 [Done] With diagnostics enabled, four core performance tests pass, including nested batches, early return, unwind, worker publication and saturation.
- 🟠 [Next] Audit independently spawned threads and platform-specific command boundaries outside these wrappers. Batch timers include synchronous waits/dialogs and are not pure CPU time or input-to-present latency. Already-published render/job snapshots are drained and are not counted twice.

Verification after job/session integration: diagnostics-enabled performance tests 4 passed; diagnostic and ordinary host `cargo check` passed; core/renderer all-target Clippy with heap profiling and Skia passed with warnings denied; formatting and whitespace checks passed. Adobe comparison self-check produced zero differences, and a wrong-size input was rejected without resampling. Earlier complete core/renderer suite: 290 + 228 passed, with platform/manual GPU tests separately executed as recorded above.

### Native incremental invalidation and input diagnostics (2026-10-08)

- 🟢 [Done] `Journal::layer_sequence` rolls up every layer/object event without allocating a target during lookup. Removed layer entries are released; recreation advances the sequence. Unchanged layers remain identifiable after journal rollover, and cloned documents keep independent journal identities.
- 🟢 [Done] `native_bezier::Cache::synchronize` checks the journal identity, cursor and document revision before visiting SVG sources. Stable frames now visit zero SVG layers. A changed revision still performs the conservative source check, and missing history still performs a rebuild.
- 🟢 [Done] Native layer verification reuses certified geometry or cached fallback reasons when source identity and layer sequence are unchanged, even when other layers have changed. Changed source, changed visibility and mutable forks invalidate that shortcut.
- 🟢 [Done] Geometry now records its layer owner; a bounded layer-to-cached-object index prunes object/layer removals from journal IDs. Structural changes invalidate only the affected layer's verification and draw list. Adding/deleting an object or deleting a layer no longer gathers every object's ID; cold starts, history gaps and unjournaled source mismatches retain conservative full rebuilding.
- 🟢 [Done] Actual local GPU regression validates identical incremental/full-render pixels for move, Undo/Redo, deletion/Undo and cached layer removal. Geometry and uniforms remain reused in stable frames; source-layer and live-object scans are asserted zero.

Local host microbenchmark (test build, 20,000 calls, no GPU commands inside the timed loop):

| SVG layers | Previous source-walk loop | New revision-check synchronization | New source-layer visits |
| --- | ---: | ---: | ---: |
| 1 | 153,500 ns | 143,875 ns | 0 |
| 16 | 1,367,167 ns | 143,875 ns | 0 |

These are total loop times, not application FPS or 60-second Release workloads. The old algorithm is reproduced explicitly in the test; the after measurement calls the actual modified synchronization path. The 16-layer synchronization is approximately 9.5 times faster in this sample.

- 🟢 [Done] Added opt-in synchronous input/command markers and a bounded 512-sample handler-to-present-call tracker. macOS pointer actions and native edit commands propagate markers when scheduling or rendering a frame. Coalesced redraws preserve first/latest event IDs; duplicate marking is ignored. Present logs can be correlated to the same GPU timestamp stream/frame when a timestamp slot is available.
- 🟢 [Done] Logs expose first/latest host latency plus rolling p50/p95/p99 and counts above 16.67/8.33 ms. No GPU wait/readback is added. Disabled diagnostics allocate no latency tracker and take no input clock readings.
- 🟠 [Next] Measure full application interactions with these logs under the specified controlled Release protocol. These metrics end at the host present call and exclude OS input dispatch, compositor/display scanout and physical input-to-photon latency; they do not prove 60/120 fps or that a deferred content worker has completed.

### Native curve coverage and encoded premultiplication (2026-10-08)

- 🟢 [Done] Replaced the orientation-independent distance ramp with integration of a locally straight boundary over the physical square pixel. This improves diagonal/cubic edge coverage without extra texture transfers, geometry rebuilds or CPU image composition.
- 🟢 [Done] General curve output now obeys the same encoded-sRGB premultiplied RGBA contract as the compatibility renderer and exact-rectangle branch. Previously linear RGB was multiplied by alpha before the sRGB target encoded it, producing encoded RGB greater than alpha at transparent edges. The GPU test now checks the premultiplied RGB bound at six zoom levels.

Measured alpha error versus the same compatibility renderer and fixture on the same local GPU:

| Zoom | Previous mean / max | New mean / max | Previous / new pixels over 1 LSB |
| --- | ---: | ---: | ---: |
| 25% | 0.099976 / 54 | 0.030945 / 26 | 76 / 64 |
| 50% | 0.176697 / 50 | 0.081421 / 40 | 142 / 125 |
| 100% | 0.327393 / 48 | 0.129272 / 34 | 280 / 232 |
| 150% | 0.261963 / 54 | 0.117249 / 50 | 200 / 168 |
| 200% | 0.073242 / 42 | 0.027954 / 34 | 54 / 44 |
| 400% | 0 / 0 | 0 / 0 | 0 / 0 |

- 🟢 [Done] Actual GPU regression passes after the shader change, including qualified rectangle/stroke comparisons, retained geometry reuse, transform/Undo/Redo, structural deletion/restoration and source-mismatch fallback.
- 🟠 [Next] Curvature/corner coverage still differs by up to 34 LSB at 100% and 50 LSB at 150%; retain the existing normal-zoom cubic quality gate. These results improve one diagnostic fixture and do not establish exact Adobe equivalence or a measured GPU speedup.

Verification for this batch: core/renderer heap-profile+Skia suite passed (292 + 230 tests in that run), five diagnostics-enabled core performance tests passed, actual GPU incremental/coverage test passed, and diagnostic host compilation passed. Formatting/whitespace checks passed. Clippy checks retain warnings-as-errors.


## Exact rational conics and bounded round-cap strokes (2026-10-08)

- 🟢 [Done] `native_bezier.rs` retains Skia rational quadratic conics with their weight; `native_bezier.wgsl` evaluates rational position, first/second derivatives and monotonic ray intersections. No zoom-dependent polyline subdivision or frame upload is introduced. Only finite weights in (0, 1] and at most 32 segments are admitted; dense dashes retain compatibility fallback.
- 🟢 [Done] Stroke outline preparation handles polylines, cubic strokes and round caps/joins for experimental comparison. Production display admits only the previously qualified rectangular strokes and opaque, undashed, axis-aligned round-cap straight strokes with positive uniform object scale above 150% screen zoom. At normal zoom round caps retain compatibility rendering. Unsupported curve/polyline strokes explicitly clear the ready-layer flag so the SVG fallback remains visible.
- 🟢 [Done] Actual GPU comparisons include a short round-cap line with both caps inside the output at 200% and 400%. Mean alpha error against Resvg is 0.070557 / 0.062256 of one 8-bit level; maximum error is 45 / 30. These are approximate edge comparisons, not exact Adobe parity. Encoded RGB stays premultiplied, and repeated preparation reuses geometry with zero geometry builds and identical pixels. Existing rectangle/dash six-zoom exact comparisons and move/Undo/Redo checks still pass.
- 🟠 [Next] The V-shaped polyline at 200% has mean alpha error 0.408936 and maximum 50; a cubic stroke has mean 1.093140 and maximum 144. Both therefore remain on compatibility rendering. A 400% cubic result of zero error is not sufficient to enable the general path. Full Adobe stroke comparisons, asymmetric transforms, transparent strokes and general normal-zoom quality remain unfinished.
- Tradeoff: segment storage grows from 32 to 48 bytes to carry rational weights and type metadata. Existing bounded GPU workload and cache limits remain. No application FPS, latency reduction or lower total memory claim is made for this change.
- Verification: eight focused CPU tests, the actual-GPU retained native rendering regression, renderer unit suite and renderer all-target Clippy with Skia/heap profiling. GPU readback occurs only inside the test, not the display path.


## Resumed: fixed-precision stroke outlines (2026-10-08)

- 🟢 [Done] Resumed implementation after the user-requested checkpoint. Stroke outlines now supply a fixed 16× precision matrix to Skia's `fill_path_with_paint`, rather than its default 1× setting. This matrix changes outline approximation accuracy, not object scale. It is independent of viewport zoom, so retained geometry remains reusable on zoom/translation. The 32-segment/work-budget limits remain; more complex outlines can still fall back. Added opt-in `native_stroke_outline` host timing to expose cold outline construction cost.
- 🟢 [Done] Added a contained cubic fixture (`M52 72C52 48 76 48 76 72`) whose entire stroked outline is inside the 128×128 comparison at 200% and 400%. Actual GPU mean alpha difference, averaged over the entire image in 8-bit levels:

| Fixture / zoom | Default 1× precision | Fixed 16× precision | Maximum difference before → after |
| --- | ---: | ---: | ---: |
| Original large cubic / 200% | 1.093140 | 0.433167 | 144 → 52 |
| Contained cubic / 200% | 0.379150 | 0.302368 | 76 → 70 |
| Contained cubic / 400% | 0.664307 | 0.374695 | 105 → 61 |

- 🟢 [Done] Contained-cubic regression checks enforce mean/max coverage limits and the segment ceiling. Warm preparation still builds zero new geometries and reproduces identical pixels. Existing normal-zoom rectangle/stroke exact comparisons, round caps, premultiplication and move/Undo/Redo tests remain enabled.
- 🟠 [Next] These reductions concern experimental native-stroke quality, not frame-rate improvements or full Adobe parity. General cubic/polyline strokes retain the production compatibility fallback; residual edge differences remain too large to mark phase 5 complete. The previous large cubic's zero difference at 400% was not representative because its geometry left the viewport; the contained test prevents that misleading conclusion.
- Tradeoff: fixed higher precision can increase cold outline work and segment count. No cold-build speed or memory-reduction claim is made. Limits remain bounded and the new timer supports later profiling.

- Verification for the resumed change: 231 renderer unit tests passed (47 ignored); the native rendering regression separately passed on actual GPU, including the new contained-cubic error limits. Renderer all-target Clippy with Skia/heap profiling and diff checks passed.
- Host `cargo check` with heap profiling also passed.


## Retained viewport queries and qualified analytic fill normals (2026-10-08)

- 🟢 [Done] Cache the native display query using viewport document bounds, verified source identity, journal identity and the layer's sequence. Reuse the existing candidate vector without cloning when unchanged. Static frames perform zero BVH queries; pan/zoom, layer changes, source replacement, journal replacement and drag offsets invalidate reuse. A drag never publishes its expanded selected-object candidates as a reusable idle query. Failed preparation cannot reuse an abandoned candidate list; structural removal clears associated query keys. Storage is one small key per retained layer.
- 🟢 [Done] Add direct `spatial_queries` / `spatial_query_reuses` preparation metrics and the opt-in `native_spatial_query_reuses` counter. Actual GPU regressions check zero queries/one reuse on a stable layer, and one query after dragging an offscreen object back to its unchanged document state. The offscreen object is excluded again; existing pan/high-zoom, source replacement, structural removal and Undo/Redo pixel tests remain enabled. This removes repeated searches, not all per-visible-object display work; no measured application FPS claim is made.
- 🟢 [Done] For fills with positive uniform object scale, derive the pixel coverage gradient from the closest curve point and inverse transform instead of neighboring fragment finite differences. Six-zoom actual GPU comparison improves the existing cubic's mean alpha difference at 100% from 0.129272 to 0.127625, at 200% from 0.027954 to 0.026611, and at 25% from 0.030945 to 0.026917. Maximum errors remain 34, 34 and 26 respectively. These are small edge-quality improvements, not acceptance of general normal-zoom native curves.
- 🟠 [Next] An all-stroke analytic-normal experiment increased a V-shaped join's maximum error from 50 to 78 and a round cap's from 45 to 46. It was therefore rejected for strokes. Strokes and unqualified transforms retain the previous finite-difference coverage. The contained cubic stroke's 200%/400% results remain 0.302368/0.374695 mean and 70/61 maximum alpha difference, with production compatibility fallback preserved for general strokes.

- Verification: 231 renderer unit tests passed (47 ignored in the ordinary suite); the final native regression separately passed on actual GPU, including stable-query reuse and drag invalidation. Renderer all-target Clippy with Skia/heap profiling, host check with heap profiling, formatting and diff checks passed. Retained query keys are updated in place, avoiding new key-string allocation on stable preparation.


## Phase 6 completed — retained paints and clipping (2026-10-08)

- 🟢 [Done] Linear/radial gradients share the canonical SVG baked stops, including Classic/Linear/Perceptual interpolation, midpoint, transparency and affine geometry. GPU stop buffers and shape uniforms are retained; unchanged paints upload nothing. Eligible straight strokes also accept gradients.
- 🟢 [Done] Nested clipping preserves raw path geometry and even-odd holes. Pixel-aligned rectangular masks use analytical GPU coverage. Curved, rotated or fractional clips use cropped Resvg alpha masks retained in a GPU atlas, preserving compatibility antialiasing. Integer translations reuse masks; fractional transforms or zoom can rerasterize these bounded masks. This is a hybrid clipping implementation, not a claim that all clip rasterization is native.
- 🟢 [Done] Canonical object/group order and object opacity are preserved. Multiple objects or non-unit layer opacity use a retained encoded-sRGB premultiplied GPU target; effective layer opacity is applied once after composition. Stable targets are reused without redrawing. Independent nested group opacity absent from the document model and non-normal blend modes are not newly supported.
- 🟢 [Done] Explicit bounds and fallback: at most eight clips, 32 segments per native path, 8,192 retained shapes and 1,024 × 1,024 pixels per compatibility mask crop. Geometry/paint/mask retention has a 64 MiB logical budget; isolated layer targets have a separate 64 MiB budget. These are scoped logical resource budgets, not total driver VRAM. Unsupported dither/pixel styles, mixed fill/stroke, effects, noncanonical SVG and quality-gated curves/strokes retain the existing compatibility route.
- 🟢 [Done] GPU regression compares linear/radial paints, all three interpolation methods, transparency, nested/hole/curved clips, legacy/affine geometry, gradient strokes, reversed overlap order and layer opacity at 100% and 200%. Gradient RGBA differences are at most 2 LSB; group composition at most 1 LSB; cropped clip alpha comparisons are exact. Warm reuse, integer/fractional clip changes, safe capacity fallback and existing move/Undo/Redo/deletion checks pass.

Local 128 × 128 gradient movement benchmark, 50 alternating one-pixel translations, dev profile (renderer opt-level 2):

| Preparation path | CPU median | p95 | p99 | Upload per move |
| --- | ---: | ---: | ---: | ---: |
| Changed SVG + compatibility raster + RGBA upload | 0.661 ms | 0.699 ms | 0.730 ms | 65,536 bytes |
| Retained native paint preparation | 0.017 ms | 0.024 ms | 0.040 ms | 80 uniform bytes |

Native movement rebuilt no geometry and generated no SVG in this test. Warm static preparation uploaded zero bytes. The separate forced-raster baseline is not an old-application idle benchmark: the existing compatibility route also caches static images. These timings exclude document mutation/history, GPU execution and presentation; they do not establish application FPS or Release 60-second × 5 acceptance.

Validation: 291 core and 231 renderer tests pass; 1 core and 47 renderer tests remain intentionally ignored in the ordinary suite. The targeted native GPU test was separately executed and passed. Core/renderer all-target Clippy with warnings denied and the Tauri host check with heap profiling pass. Phase 6 completion supersedes its earlier deferred status; phases 4/5 quality gates, phase 7 incremental coverage and phase 8 application/Adobe acceptance remain unfinished.


## Glyph Atlas started — 2026-10-08

- 🟢 [Done] `glyph_atlas.rs` implements a bounded renderer-side R8 coverage page and exact, collision-safe outline keys. Glyph shape, raster dimensions (up to 512 × 512), fill rule and full local transform—including fractional phase—identify coverage. Text position and color can remain instance data only when they do not change that local coverage. The caller retains prepared keys to avoid rebuilding outline identity on every lookup.
- 🟢 [Done] Compatibility tiny-skia rasterization runs only on an atlas miss; upload includes a zero gutter. A page is at most 2048² (4 MiB); outline identities have a separate 4 MiB/16,384-entry bound. Oversized/complex glyphs and full pages return a failure for future compatibility routing. Reset increments generation; callers must rebuild all referring instances. No implicit eviction of still-referenced slots is allowed.
- 🟢 [Done] Scoped hit/miss/reset/upload diagnostics and a miss raster/upload host timer. Actual GPU test verifies transferred mask bytes and gutters by test-only readback, warm no-raster lookup, invalid-data/capacity rollback and generation invalidation. Production atlas code has no readback or JavaScript frame transfer.
- 🟠 [Next] Glyph-run extraction, instance buffers, shader composition and renderer installation are not yet implemented. Keep the existing text route active until horizontal/vertical Japanese, Latin/Chinese shaping, rotations, frame clips, decorations and alpha/color ordering pass compatibility comparisons. Bitmap/COLR/SVG glyphs need explicit fallback or a separately qualified color atlas. Phase 3 remains incomplete and this foundation does not establish application latency/FPS improvement.


## Glyph Atlas: shaped runs and application pilot — 2026-10-08

- 🟢 [Done] `glyph_run.rs` consumes the existing usvg layouted glyph IDs, resolved font database and outline transforms instead of reshaping or advancing text independently. Static glyf/CFF outlines are converted to exact atlas identities; integer placements reuse masks, fractional phase is retained in the local raster transform. Transparent glyph color is instance data. Integer axis-aligned rectangular clips are intersected and applied in the GPU shader.
- 🟢 [Done] `glyph_draw.rs` and its WGSL draw ordered glyph instances from retained R8 coverage. An encoded-sRGB layer target isolates glyph composition; the shared native layer compositor applies effective layer opacity once and converts to the scene target format. No production GPU readback or large JavaScript pixel transfer is introduced.
- 🟢 [Done] `glyph_display.rs` connects a renderer-owned **opt-in pilot**, enabled with `LUMAPAINT_GLYPH_ATLAS=1` before launch. It reuses whole ready batches/targets for unchanged source and viewport. Changed content/view prepares new instances while retaining matching masks. Removed layers release targets; the target/instance/source logical payload budget is 64 MiB with 64 retained layer entries, separate from the 4 MiB atlas and bounded key storage. Exhausted atlas capacity retains compatibility rendering; automatic eviction is intentionally absent because active batches refer to slots.
- 🟢 [Done] Default application rendering is unchanged. Pilot eligibility is exactly one physical pixel per document pixel, integer canvas placement and supported text-only appearance. Drag, other densities, outline display, document clipping/active saved paths and layer effects use existing routes. Variable fonts, bitmap/COLR/SVG glyphs, text strokes/decorations, crisp-edge text hints, nonrectangular/fractional clips and isolated non-unit group opacity are rejected as complete runs. Static-font default weight variation metadata is harmless because the resolved face is already selected; genuinely variable faces remain excluded.
- 🟢 [Done] On the local GPU, Latin, Japanese/Chinese with fallback fonts, vertical Latin, translucent text, integer translation and final sRGB layer composition differ from Resvg by at most 1 RGBA LSB. The draw test mean RGBA error ranges from 0.0019 to 0.0077 LSB over the full 256 × 128 image. This is a scoped compatibility quality result, not an Adobe export comparison. Display tests cover source/opacity/view invalidation, warm no-redraw reuse, return to prior content and capacity/unsupported fallback. Returning to prior source is not a full document-history acceptance test.
- 🟠 [Next] Full GUI selection, editing and Undo/Redo acceptance; overlapping glyph/color edge cases, arbitrary font coverage and decorations; fractional sampling/zoom, dragging, variable/color fonts and application performance measurements. Source equality and cold per-glyph outline extraction still have costs. The pilot is not enabled by default, and Phase 3 remains incomplete.


## Phase 3: qualified Glyph Atlas zoom — 2026-10-09

- 🟢 [Done] Extend the opt-in application pilot from physical density 1 to **0.5, 1, 1.5 and 2 device pixels per document pixel**. Application canvas boundaries and eligible rectangular text clips must still be integer-aligned. Physical density includes display backing scale; these values do not always equal the percentage shown in the UI on a Retina display.
- 🟢 [Done] GPU comparison matrix: each density with Latin, Japanese/Chinese, vertical Latin, translucent text and integer physical translation (20 cases). All compared RGBA channels differ from direct Resvg by at most 1 LSB. App-equivalent final encoded/sRGB composition also passes at all four densities, including a 100% → 50% → 100% → 150% → 200% → 100% sequence. Each unchanged-view repeat reuses the clean layer target.
- 🟢 [Done] Keep unqualified densities, fractional canvas boundaries and unsupported clips on compatibility rendering. No interpolation approximation or clip-quality threshold was relaxed. Atlas capacity and glyph-size bounds remain unchanged; zoom requests new masks only for different local raster identities.
- 🟠 [Next] This closes the **four-density pilot implementation/renderer-verification item**, not universal zoom or Phase 3. Arbitrary/fractional zoom/pan and clip coverage, selection dragging, variable/color glyphs, full GUI editing/history and controlled application latency/transfer measurements remain unfinished. Default rendering remains unchanged.

Validation: 234 renderer tests and all three explicitly executed Glyph Atlas GPU regressions passed; renderer all-target Clippy with Skia/heap profiling and warnings denied, Tauri heap-profile check, formatting and whitespace checks passed. Timing/FPS improvement has not been established by these quality tests. The existing compatibility application path also caches static frames, so zero warm atlas redraws must not be presented as a before/after application speedup.


## Phase 3: fractional rectangular frame clips — 2026-10-09

- 🟢 [Done] Single fractional axis-aligned rectangular clips are baked into boundary glyph coverage using the same clear/invert/apply-mask operation as Resvg. Local clip coordinates are part of exact atlas identity. Interior glyphs keep their unclipped keys; boundary masks rasterize/upload only on a miss. Shader clip bounds round outward so partially covered edge pixels are retained rather than discarded.
- 🟢 [Done] Preserve composition semantics with conservative fallback for overlapping boundary-glyph ink bounds and for nested fractional masks. A bounded 64-pixel spatial grid (400,000 cell references maximum) avoids an unbounded pairwise scan. This is a compatibility gate, not an implementation of general overlapping glyph union. Clip paths with unqualified opacity/paint or crisp-edge hints also retain compatibility.
- 🟢 [Done] Extend the opt-in application densities to **0.25 / 0.5 / 0.75 / 1 / 1.5 / 2**. Physical canvas boundaries remain integer-aligned. The latest GPU matrix attempted 42 cases: 41 native comparisons passed with at most 1 RGBA LSB difference; a small vertical case retained compatibility because of boundary overlap. Explicit tests cut through glyph ink on all six densities, with opaque and translucent color. Final application-equivalent sRGB composition and warm target reuse pass at each density.
- 🟢 [Done] Exact-key/partial-alpha tests, overlapping-boundary and nested-fractional fallback regressions, all 234 renderer tests and the three explicit Glyph Atlas GPU regressions pass. All-target renderer Clippy with warnings denied passed. Glyph-sized CPU coverage is bounded by the existing 512² limit; no full-document CPU frame or production GPU readback was added.
- 🟠 [Next] A diagnostic **1.25-density Japanese/Chinese case still differs by up to 15 LSB**. The broader overlap gate did not remove that discrepancy; its root cause remains unconfirmed. This density remains excluded from application eligibility, with an explicit rejection regression. Do not claim arbitrary zoom complete or relax the maximum-error gate based on its low average error.
- 🟠 [Next] Fractional canvas boundaries/pan, nested fractional/group masks, arbitrary density, general overlapping glyph composition and GUI editing/history acceptance remain open. Default rendering remains unchanged. This entry supersedes the earlier single-density/integer-frame-only pilot limits, not Phase 3's unfinished status.


## Phase 3: 1.25-density Japanese quality correction — 2026-10-09

- 🟢 [Done] Fix the confirmed transform-order discrepancy. Canonical usvg rendering transforms font-unit outlines into layout coordinates first, then applies the outer SVG/view transform. The atlas had combined these transforms and rasterized original font-unit geometry. Apply the glyph outline transform to the path first and retain the outer transform as the atlas raster transform, matching compatibility rounding before scan conversion.
- 🟢 [Done] The previously failing Japanese/Chinese 1.25-density fixture improves from **maximum 15 RGBA LSB difference to maximum 1 LSB**, without changing the quality tolerance. The correction is about geometry/coverage precision, not lower-resolution rendering. Layout-resolved outline coordinates participate in exact cache identity; warm source/view reuse and integer physical pan key reuse still pass. This does not establish universal mask reuse for committed text movement or differently positioned repetitions.
- 🟢 [Done] Enable physical density **1.25** in the opt-in application route, alongside 0.25 / 0.5 / 0.75 / 1 / 1.5 / 2. Final sRGB composition, zoom return/reuse and the explicit 1.25 eligibility regression pass. This supersedes the earlier recorded 1.25 rejection/unconfirmed cause.
- 🟢 [Done] Expanded GPU matrix attempts 49 cases: 48 native comparisons remain at maximum 1 RGBA LSB and one conservative fractional-boundary fallback is retained. Cases include multilingual fallback, vertical Latin, translucent text, integer translation and fractional clips cutting through opaque/translucent glyphs. Test diagnostics identify differing pixel/channel and containing glyph if the unchanged maximum-error gate fails again.
- 🟠 [Next] Phase 3 remains unfinished: arbitrary densities/fractional canvas/pan, nested/group masks and general overlap, variable/color fonts and decorations, full GUI edit/history acceptance, default enablement and application latency/transfer benchmarks. No complete Adobe/font parity or FPS claim is made.

Validation for this correction: 234 renderer tests passed; all three Glyph Atlas GPU regressions explicitly passed. Renderer all-target Clippy with Skia/heap profiling and warnings denied, Tauri heap-profile check, formatting and diff whitespace checks passed.


## Phase 3: fractional interior placement — 2026-10-09

- 🟢 [Done] Allow fractional physical canvas placement/pan at the seven qualified densities when glyph coverage remains inside the canvas. Finite canvas bounds and conservative glyph-mask/frame bounds determine eligibility. Integer canvas boundaries retain binary clipping; fractional boundary-crossing ink retains compatibility rendering. The opt-in route remains disabled by default.
- 🟢 [Done] Rejecting preparation now removes the layer from the ready set, including rejection after successful preparation in the same frame. Previously retained targets cannot suppress compatibility rendering after an eligibility change.
- 🟢 [Done] Application-equivalent real-GPU comparisons at three fractional placements, including negative pan, opaque/translucent layers and returning to a prior view show maximum 1 RGBA LSB difference. Warm repeated preparation does not redraw. Top/left and bottom/right boundary-crossing fixtures explicitly assert fallback. The maximum-error tolerance remains 2 LSB.
- 🟠 [Next] Fractional canvas-edge coverage remains unfinished. An analytical rectangular root clip was experimentally compared and differed by up to 32 LSB; it was removed from production. Exact compatibility coverage must be qualified before enabling boundary-crossing ink. Interior-placement acceptance does not close arbitrary pan/density, general overlap/masks, full GUI/history or performance benchmarks.

Validation: all three explicit Glyph Atlas GPU regressions passed; the default renderer suite passed 205 tests. Renderer all-target Clippy with Skia/heap profiling and warnings denied, the Tauri heap-profile check, formatting and diff whitespace checks passed. The Skia/heap-profile renderer suite passed 234 tests (50 GPU/environment-dependent tests ignored; the three atlas GPU regressions were run explicitly).


## Phase 3: canvas-boundary glyph masks — 2026-10-09

- 🟢 [Done] Extend exact glyph identity with a separate canvas mask. Frame coverage and canvas coverage multiply in order rather than intersecting rectangles or replacing one another. Only boundary masks rasterize/upload on an atlas miss, within the existing 512² glyph-mask, 4 MiB texture and 4 MiB key budgets. Interior keys remain unchanged; no full-document CPU frame or production readback is introduced.
- 🟢 [Done] Reuse the bounded overlap gate after adding canvas masks. Per-glyph root clipping is enabled only when affected ink cannot overlap other glyphs; otherwise retain compatibility composition. Unclipped runs skip grid allocation, and interior-only root placement skips a second overlap scan. Setting canvas identity mutates the bounded clip field without cloning the outline.
- 🟢 [Done] Identify an independent viewport-edge coverage discrepancy: at 0.75 density, the compatibility renderer clips a sloping outline at viewport row 127 before rasterization, while the atlas crops its complete raster afterwards. The differing pixel has full canvas-mask coverage (255), so the error is not a root-mask error. Fractional placement with viewport-crossing ink now explicitly falls back; the quality tolerance remains unchanged.
- 🟢 [Done] The real-GPU matrix compares 96 canvas-boundary cases across all seven qualified densities (14 / 14 / 14 / 14 / 14 / 14 / 12 cases respectively), with maximum 1 RGBA LSB difference. It covers four document edges, independent frame/canvas clipping, opaque/translucent composition and repeat-view reuse. Each density explicitly compares boundary ink, avoiding an all-fallback pass. The diagnosed viewport-bottom case explicitly rejects eligibility. All three atlas GPU regressions pass without relaxing the 2-LSB tolerance.
- 🟠 [Next] Exact viewport-cropped outlines, general overlapping glyph/root composition, nested/group masks, arbitrary densities, broader font/decorations, GUI edit/history and application latency/transfer acceptance remain open. The atlas stays opt-in; this work does not establish full Phase 3 completion or default enablement.

Validation: 236 renderer tests passed with Skia/heap profiling (50 environment/GPU tests ignored); the three Glyph Atlas GPU regressions were explicitly run and passed. The new CPU regressions cover mask multiplication/identity and post-mask overlap/viewport eligibility. All-target renderer Clippy with warnings denied, Tauri heap-profile check, formatting and diff whitespace checks passed. The combined real-GPU logs contain 191 pixel comparisons, all with maximum 1 RGBA LSB difference; 96 are canvas-boundary cases.


## Phase 3: bounded clip-overlap candidate checks — 2026-10-09

- 🟢 [Done] Split each spatial-grid cell into all-glyph and clipped-glyph candidate lists. An unclipped glyph compares only with earlier clipped glyphs; a clipped glyph compares with all earlier glyphs. This preserves the previous overlap eligibility rule while avoiding comparisons between two unclipped glyphs. The existing 400,000 cell-reference bound remains; both candidate lists are bounded by it.
- 🟢 [Done] The synthetic 4,096-unclipped-plus-one-disjoint-clipped regression passes: candidate comparisons fall from 8,390,656 to 4,096, with unchanged eligibility. Actual overlapping boundary ink still rejects. The adversarial candidate test stops at 400,001 comparisons and falls back. This is a deterministic work-count comparison, not an application timing/FPS claim.
- 🟢 [Done] Add a 400,000 candidate-comparison budget and counters `glyph_clip_overlap_comparisons` / `glyph_clip_overlap_budget_fallback`. Exhaustion returns to compatibility before atlas preparation/upload, rather than allowing quadratic cold-path work. Warm layer reuse still avoids the overlap scan entirely.
- 🟠 [Next] Viewport-cropped outline rendering remains unfinished. Cropping individual raster surfaces introduced errors in Japanese/multiple-glyph and vertical/edge fixtures; both general and isolated-visible-glyph experiments were removed. The complete glyph raster and previous viewport-edge compatibility gate remain. General path union/group masks and full GUI/performance/default-enable acceptance remain open.

Validation: 238 renderer tests passed with Skia/heap profiling (50 environment-dependent tests ignored); all three Atlas GPU regressions explicitly passed, preserving the 96-case canvas-edge matrix at maximum 1 RGBA LSB. All-target Clippy with warnings denied, Tauri heap-profile check, formatting and diff whitespace checks passed. ROADMAP status markers now follow the requested emoji convention.


## Shared parsed-text cache: shorter lock scope — 2026-10-09

- 🟢 [Done] Compute a bounded sampled fingerprint before acquiring the shared cache lock. At most 128 source bytes participate, plus length; long inputs therefore do not introduce a new whole-source hashing pass on warm lookup. Fingerprints only filter candidates: full source equality remains mandatory, including intentional or natural sampling collisions.
- 🟢 [Done] Prepare the owned source/tree entry before locking. Return evicted, duplicate or rejected entries to the caller and release them after the guard is gone. Large source allocation/copy and final shaped-tree destruction no longer occur inside insertion's shared critical section. Entry count (32), accounted retention (32 MiB), exact LRU promotion, try-lock fallback and resource-heavy bypasses remain unchanged. This budget is an estimate, not process RSS; in-flight Arc users may retain evicted resources.
- 🟢 [Done] Seven cache regressions pass: a 32-entry long-prefix fixture checks zero full source comparisons on a fingerprint miss and one on a warm hit, compared with up to 32 equality checks previously. Forced and genuine sampling collisions still return the correct tree; a Weak ownership regression verifies that evicted resources outlive the cache lock and are freed when the returned retirement list is dropped. Existing multilingual/vertical/rotated cache-versus-fresh pixel equality, concurrent-reader equality, LRU/size limits and bypass tests pass.
- 🟢 [Done] Add `parsed_text_fingerprint` timing and `parsed_text_cache_source_comparisons` counters. Fingerprint collisions can still require multiple full comparisons; this change does not claim eliminated lock contention, application latency improvement or FPS from synthetic work counts alone.
- 🟢 [Done] Final checks pass: 240 renderer tests with Skia/heap profiling (50 environment/GPU tests ignored), all-target Clippy with warnings denied, Tauri heap-profile compilation, formatting and diff whitespace checks. The seven cache regressions are included in the full suite.
- 🟠 [Next] Application-level worker contention and latency measurements, exact viewport-edge/path-union rendering and broader Phase 3 acceptance remain unfinished. Atlas default enablement remains unchanged.


## Shared parsed-text cache: Release lookup measurements — 2026-10-09

- 🟢 [Done] Add an explicitly ignored, Release-only lookup benchmark. It compares the former linear source-equality algorithm against fingerprint filtering on the same current cache storage, including Arc cloning and LRU promotion. It uses seven alternating-order batches of 5,000 operations, with metrics disabled. This is an algorithm control, not a historical application binary or a lock-contention measurement. Source comments create equal-length, long-prefix keys without changing visible content.

| Entries / source bytes | Warm hit: linear → current (ns/op) | Miss: linear → current (ns/op) |
| --- | ---: | ---: |
| 1 / 1,023 | 24.0 → 68.1 | 21.5 → 44.5 |
| 8 / 8,191 | 1,392.7 → 298.2 | 1,439.8 → 47.6 |
| 32 / 65,535 | 74,779.4 → 2,684.3 | 77,410.5 → 95.8 |
| 4 / 510,999 | 60,897.1 → 15,057.9 | 51,995.5 → 56.9 |

- 🟢 [Done] Record the small-workload tradeoff: one short-source hit costs about 44 ns more, while the 32-entry long-prefix hit is about 28× faster in this fixture. Keep bounded fingerprints outside the shared lock rather than adding a second lock acquisition or full-source hashing to remove that small fixed cost. No absolute speed gate is imposed on machine-dependent timings. Reported values are medians of batch means, not per-operation p95/p99; there is no separately excluded warm-up batch.
- 🟠 [Next] Application-level contention, input-to-screen latency and production workload distributions remain unmeasured. These lookup results do not establish FPS, GPU performance, or full Phase 3 completion.

Reproduce: `env -u LUMAPAINT_RENDER_METRICS cargo test --release -p lumapaint-renderer benchmark_parsed_cache_lookup --lib -- --ignored --nocapture`.

Backend context on the same Release run (`bench_text_resolution`, Resvg, 20 warm samples): five lines have median 2.84 / 1.29 / 0.63 ms at 2048 / 1024 / 512 pixels; 45 lines have median 27.45 / 12.29 / 6.19 ms. Editing one of two frames averages 36.5 ms for full preparation, 21.9 ms for cached full-canvas preparation and 14.0 ms for retained-frame preparation (three edits, with cached/full pixel equality asserted). Retained payload is 5,352,024 bytes versus 16,777,216 full-canvas bytes. These are current CPU/backend baselines, without a pre-change rendering control; they do not quantify this lookup change's end-to-end benefit. GPU reuse, GUI latency and 60-second repeated application measurements are separate unfinished checks.

Validation for this measurement item: Release lookup benchmark passed; seven cache regressions passed in the compiled Release test executable (benchmark ignored in that run). The subsequent edit only changes the benchmark's debug-build guard from a constant assertion to a conditional panic; current all-target Skia/heap-profile Clippy, formatting and diff whitespace checks pass. A separate debug test invocation waited on another build's directory lock; it adds no validation evidence. No new GPU/application acceptance is claimed.


## Shared parsed-text cache: contention attribution — 2026-10-09

- 🟢 [Done] Separate ordinary cache misses from nonblocking lookup/insertion lock contention and poisoned-lock fallback. Add `parsed_text_cache_lookup_contended`, `parsed_text_cache_insert_contended` and corresponding `_poisoned` counters. Existing miss totals still include failed lookups, preserving their meaning; contention counters identify the subset that can trigger redundant shaping.
- 🟢 [Done] Add inclusive host timers for shared lookup, insertion and retired-entry destruction. Insertion ends after guard release and before retirement starts; parsing, source allocation and retirement remain outside the critical section. Metrics remain opt-in and thread-local, with existing batch publication. No blocking lock, GPU readback or JS frame transfer is introduced.
- 🟢 [Done] A deterministic held-lock regression invokes lookup and insertion while the same thread owns the mutex. Both return without waiting; the rejected tree is freed, retained entry/count/charge stay unchanged, and lookup after release returns the original Arc. With metrics enabled, the test checks exactly one lookup contention and one insertion contention, plus the three timing labels.
- 🟠 [Next] Capture these diagnostics during real text selection/dragging and concurrent worker workloads before choosing duplicate-parse suppression or scheduling changes. Instrumentation itself is not a demonstrated speedup; application latency and broader Phase 3 acceptance remain unfinished.

Targeted validation: eight cache regressions pass with `LUMAPAINT_RENDER_METRICS=1`, including concurrent/pixel-equality tests and the new diagnostic assertions (manual lookup benchmark ignored). All-target renderer Clippy with Skia/heap profiling and warnings denied, formatting and diff whitespace checks pass. Reproduce diagnostics with `LUMAPAINT_RENDER_METRICS=1 cargo test -p lumapaint-renderer vector::parsed_text::tests --lib`. Timers are inclusive host time; retirement may be empty, and counters are not a contention percentage until related totals are collected for the same published batches.

- 🟢 [Done] Contention diagnostics validation: the full renderer suite with Skia/heap profiling passes 241 tests (51 ignored, including the manual Release benchmark and environment/GPU cases). No GPU test was newly run for this cache-only change.


## Shared parsed-text cache: parallel readers — 2026-10-09

- 🟢 [Done] Add an ignored Release worker benchmark using the actual `parse` route, eight prewarmed multilingual SVG sources, 1 / 4 / 8 threads, 200 lookups per thread and three batches per condition. Aggregate thread-local diagnostics explicitly after joining workers. Metrics are enabled; the timer starts before barrier release and ends after joins, including scheduling. This is a synthetic backend workload, not GUI latency, GPU time, or an instrument-free throughput benchmark.
- 🟢 [Done] Baseline exclusive-mutex measurements expose redundant parsing: eight workers on 65,691-byte sources produce 13–15 contended lookups / SVG parses per 1,600 operations and 10.90–12.76 ms batch wall time. One worker on the same fixture produces zero parses and 0.298–0.309 ms per 200 operations. This is a confirmed benchmark bottleneck, not evidence that real application selection uses eight workers.
- 🟢 [Done] Use a nonblocking shared read lock for source comparison and Arc acquisition. Attempt LRU promotion only after releasing the read guard. If promotion is contended, preserve the successful hit and count `parsed_text_cache_promotion_skipped`; do not reshape text. Exclusive insertion and write-lock contention still fall back without waiting. Caller-owned Arcs survive eviction. Exact source equality, collision checks and the 32-entry / 32-MiB accounted limits remain.
- 🟢 [Done] LRU promotion is now best effort during contention, rather than guaranteed per successful shared access. Sequential access preserves promotion order; a skipped promotion can evict a recently used entry sooner. This bounded-cache tradeoff changes reuse policy, not rendering output. These rules supersede earlier claims of exact shared LRU promotion.
- 🟢 [Done] Nine cache regressions pass with metrics enabled, including a held shared-reader case that preserves the successful Arc hit and records exactly one skipped promotion with no lookup contention. Existing exclusive-lock fallback/accounting, multilingual/vertical/rotated pixel equality, collision and concurrent-reader tests pass.
- 🟠 [Next] Real application latency/worker capture, cold insertion storms, budget/eviction workloads and broader Phase 3 acceptance remain unfinished. This benchmark does not establish default atlas enablement or FPS.

Reproduce the parallel experiment: `LUMAPAINT_PARALLEL_TEXT_CACHE=1 LUMAPAINT_RENDER_METRICS=1 cargo test --release -p lumapaint-renderer benchmark_shared_parsed_cache_workers --lib -- --ignored --nocapture`. Omit `LUMAPAINT_PARALLEL_TEXT_CACHE` to measure the default exclusive-mutex route in the same build. Use separate fresh processes; the cache mode is selected once at initialization.

Promotion uses an entry identity allocated on insertion preparation, rather than comparing the SVG source a second time under the write lock. Identity is separate from the sampled fingerprint and tree Arc: distinct sources can deliberately share both in tests. A dedicated collision/shared-tree regression verifies that promotion moves the correct source. The initial read-lock experiment repeated full source comparison during promotion and showed a serial-workload regression; that extra comparison was removed before final acceptance. Timing comparisons across builds are short, scheduler-sensitive exploratory measurements, not controlled application speedup claims.

Final identity-version measurements: the eight-worker / 65,691-byte condition records 6–9 redundant parses in the first final run and 2–8 in a repeat after the launched builds completed, versus 13–15 in the mutex baseline. The repeat's wall times are **9.894 / 19.814 / 21.991 ms** (sorted), versus baseline **10.904 / 12.134 / 12.759 ms**. Serial 200-operation batches also rise from 0.298–0.309 ms to 1.037–1.290 ms. Thus fewer retries are observed, but **a wall-time speedup is not established and some measurements regress**. The runs are separate short scheduler-sensitive batches with instrumentation; no same-process interleaved baseline or stable machine-load/frequency control is present. Do not convert these exploratory measurements into an application speedup claim. Matched repeated latency/serial-overhead qualification remains Next.

Final validation: ten cache regressions pass with metrics enabled (two manual benchmarks ignored), including identity/collision promotion. Final all-target Skia/heap-profile Clippy with warnings denied passes. The earlier parallel-reader version passed 242 full renderer tests before the identity-only refinement; final-suite evidence is recorded separately. Formatting and diff whitespace checks pass.

- 🟢 [Done] **Final acceptance decision: parallel reads remain opt-in**, enabled only with `LUMAPAINT_PARALLEL_TEXT_CACHE=1`. The default remains the prior nonblocking exclusive mutex with exact sequential LRU promotion. Only the experimental route uses best-effort promotion. Cache mode is selected once at initialization; only one cache/budget is allocated, with no doubled global retention. This final decision supersedes any earlier implication that parallel reads became the standard path.
- 🟠 [Next] Same-build alternating baseline/experimental measurements under stable host load, including serial overhead, before default enablement. Retry reduction alone does not satisfy performance acceptance.

- 🟢 [Done] Final gated implementation checks: ten metrics-enabled cache regressions pass separately on the default and opt-in parallel routes; the final default renderer suite passes 214 tests (45 ignored). All-target Skia/heap-profile Clippy with warnings denied, formatting and diff whitespace checks pass. Earlier pre-gate Release suite also passed 214 tests. No new explicit Glyph Atlas GPU qualification or application FPS claim is made.


## Shared parsed-text cache: matched backend benchmark — 2026-10-09

- 🟢 [Done] Refactor the private parse body into `parse_with_cache`; production still initializes its single configured cache once, then uses this same body. The manual Release worker benchmark now calls that body with explicit independent serial and parallel caches in one process. Environment flags no longer choose a single backend for this benchmark; both backends always run. Existing separate-run measurements above remain exploratory history.
- 🟢 [Done] Prewarm identical eight-source fixtures for both caches, alternate backend order each round and exclude round zero. Compare seven measured batches for each source size / worker count, with 200 lookups per worker. Report worker-aggregated counters and inclusive host CPU timers; wall time still includes barrier release, worker execution and joins. Worker creation precedes the timer, although thread startup scheduling can remain.
- 🟠 [Next] Stable-host repeated application latency, serial-overhead/default-enable acceptance and cold/eviction workloads. The matched comparison reduces ordering/build differences but does not remove OS scheduling, frequency or background-task effects. Metrics stay enabled and durations are not GPU or physical-display latency.

Reproduce both modes in one binary: `LUMAPAINT_RENDER_METRICS=1 cargo test --release -p lumapaint-renderer benchmark_shared_parsed_cache_workers --lib -- --ignored --nocapture`. No parallel-cache environment flag is needed for this benchmark. Production parallel reads remain opt-in.

- 🟢 [Done] Execute the matched Release comparison twice. Each run passes all 96 batches (two sizes × three worker counts × eight rounds × two backends); only the 84 post-warm-up batches enter summaries. All batches assert hit + miss totals against submitted operations.

| Source bytes / workers | First median ms: serial → parallel | Repeat median ms: serial → parallel |
| --- | ---: | ---: |
| 1,179 / 1 | 0.151 → 0.131 | 0.851 → 0.580 |
| 1,179 / 4 | 11.519 → 8.465 | 19.374 → 17.624 |
| 1,179 / 8 | 15.160 → 13.434 | 37.528 → 37.513 |
| 65,691 / 1 | 2.236 → 1.708 | 1.870 → 5.214 |
| 65,691 / 4 | 19.411 → 14.390 | 41.112 → 18.150 |
| 65,691 / 8 | 46.266 → 20.654 | 69.164 → 35.704 |

- 🟢 [Done] Document the acceptance limit rather than enabling the experiment: long-source eight-worker medians favor parallel reads in both runs (46.266 → 20.654 ms; 69.164 → 35.704 ms), but parallel maxima reach 103.052 / 187.063 ms, and the repeat's single-worker long-source median regresses (1.870 → 5.214 ms). Short-source eight-worker repeat medians are essentially equal. A single favorable median is insufficient for default enablement.
- 🟠 [Next] Reuse persistent workers instead of recreating threads each batch, separate cache-operation timing from scheduling delay, stabilize host load and extend repetitions before latency/default-enable acceptance. Thread creation is outside the current timer, but startup scheduling still enters it. Real GUI and long-duration application benchmarks remain open.

Validation: the matched benchmark passes twice; ten metrics-enabled cache regressions pass on the default route and ten on the opt-in route after this refactor. Diff whitespace checks pass. All-target Skia/heap-profile Clippy with warnings denied is **not passing in this worktree**: unrelated concurrent changes report `paint_cache.rs:102` unused `pixel`, `channel_paint.rs:139` constant `chunks_exact_mut`, and `channel_paint.rs:78` items after a test module. No finding is reported in the changed cache module. These files were not edited for this item; earlier Clippy-pass entries do not describe this current snapshot.


## Shared parsed-text cache: persistent-worker attribution — 2026-10-09

- 🟢 [Done] Reuse the same worker threads across all sixteen serial/parallel batches of each source-size / worker-count condition. All workers complete a readiness barrier before measurement. Round zero remains excluded; subsequent backend order alternates. Threads stop and join only after all batches, outside measured time. Production workers/cache selection remain unchanged.
- 🟢 [Done] Dispatch one tagged backend job to each worker and collect exactly one result per worker before starting another batch. Record `wall_ms` from dispatch start to result collection, `worker_max_ms` as the longest worker's inclusive host operation duration, and `dispatch_max_ms` as the longest interval from sending its job to receiving it. Snapshot aggregation happens after wall timing. These quantities overlap and must not be added; worker elapsed still includes OS preemption, and dispatch delay is the test queue's delay, not GUI/OS input latency.
- 🟠 [Next] Stable-host tail/serial-overhead acceptance, production scheduling and real GUI latency measurements. Persistent threads remove per-batch startup/join noise but do not eliminate background-load/frequency differences or queue wakeup scheduling. The parallel cache remains opt-in.

- 🟢 [Done] The persistent-worker Release benchmark passes all 96 batches; the 84 post-warm-up batches are summarized below. These measurements precede the subsequent newest-entry fast path.

| Source bytes / workers | Median wall ms: serial → parallel | Maximum wall ms: serial → parallel |
| --- | ---: | ---: |
| 1,179 / 1 | 0.056 → 0.056 | 0.096 → 0.120 |
| 1,179 / 4 | 9.356 → 8.558 | 11.495 → 9.808 |
| 1,179 / 8 | 11.704 → 11.880 | 41.102 → 33.762 |
| 65,691 / 1 | 0.806 → 0.790 | 1.093 → 4.484 |
| 65,691 / 4 | 17.550 → 11.817 | 19.841 → 17.132 |
| 65,691 / 8 | 37.996 → 8.476 | 46.241 → 24.142 |

The long-source / eight-worker fixture records 14–25 parses per serial batch and 0–11 per parallel batch. Its parallel maximum wall time is 24.142 ms, maximum worker elapsed 24.114 ms and maximum dispatch delay 1.857 ms (maxima may occur in different batches). The short-source / eight-worker parallel maximum wall time is 33.762 ms, with a 33.741 ms worker elapsed. These show substantial delay remains inside inclusive worker execution; this can include parsing and OS preemption, not just cache lookup. They do not isolate pure CPU time or prove that dispatch scheduling is absent. Seven batches per mode are insufficient to establish application p95/p99 or FPS. The standard backend remains unchanged.

## Shared parsed-text cache: skip redundant MRU promotion — 2026-10-09

- 🟢 [Done] In the opt-in parallel route, recognize that a successful read already occupies the newest position while holding the read guard. Return the exact Arc directly after releasing that guard, without attempting an exclusive lock or mutating the LRU vector. Non-newest hits retain identity-based best-effort promotion. The default serial route remains unchanged.
- 🟢 [Done] Count `parsed_text_cache_promotion_unneeded`. A held-reader regression checks 200 repeated newest-source hits, unchanged retained count/bytes and Arc identity; metrics must record 200 unneeded promotions and zero skipped/contended attempts. Adjust the existing blocked-promotion regression to use a genuinely non-newest entry, preserving that separate behavior's coverage.
- 🟢 [Done] Repeated-source backend timing recorded in the subsequent two-run comparison. The persistent-worker table above predates the fast path; use the later table to assess it.
- 🟠 [Next] Production-workload latency/FPS acceptance, qualified default enablement and stable-host application measurements remain open.

- 🟢 [Done] Final newest-entry fast-path checks: eleven metrics-enabled cache regressions pass separately on the default and opt-in parallel routes (two manual benchmarks ignored). Formatting and diff whitespace checks pass. The persistent-worker Release benchmark passed before this fast-path change; no post-change timing or new GPU/application acceptance is claimed.
- 🟢 [Done] Resolve the three `paint_cache.rs` / `channel_paint.rs` warnings in the subsequent lint-cleanup item. All-target Skia/heap-profile Clippy with warnings denied passes without suppression; earlier failures describe their historical snapshots.


## Shared parsed-text cache: repeated-source timing and clean lint checks — 2026-10-09

- 🟢 [Done] Extend the persistent-worker comparison to one repeated source and eight rotating sources, with identical prewarmed cache entries, alternating backend order and excluded warm-up. Run the final newest-entry fast path twice: 192 batches per run, 168 post-warm-up batches summarized. Metrics remain enabled; each worker performs 200 operations. The table reports medians of batch wall times, not per-input or per-operation latency.

| Active sources / bytes / workers | First median ms: serial → parallel | Repeat median ms: serial → parallel |
| --- | ---: | ---: |
| 1 / 1,179 / 1 | 0.064 → 0.066 | 0.083 → 0.085 |
| 1 / 1,179 / 4 | 8.350 → 0.292 | 7.898 → 0.328 |
| 1 / 1,179 / 8 | 10.801 → 0.635 | 15.662 → 0.604 |
| 1 / 65,691 / 1 | 0.527 → 0.519 | 1.228 → 1.268 |
| 1 / 65,691 / 4 | 12.795 → 0.845 | 22.586 → 1.203 |
| 1 / 65,691 / 8 | 32.841 → 1.171 | 36.853 → 2.097 |
| 8 / 1,179 / 1 | 0.063 → 0.063 | 0.110 → 0.191 |
| 8 / 1,179 / 4 | 5.003 → 7.761 | 9.130 → 11.411 |
| 8 / 1,179 / 8 | 12.649 → 13.194 | 14.033 → 24.680 |
| 8 / 65,691 / 1 | 0.695 → 0.700 | 1.127 → 1.129 |
| 8 / 65,691 / 4 | 14.952 → 11.345 | 18.874 → 17.375 |
| 8 / 65,691 / 8 | 36.423 → 1.085 | 35.881 → 13.510 |

- 🟢 [Done] Every measured one-source parallel batch records zero SVG parses and exactly one unneeded-promotion counter per operation: 200 / 800 / 1,600 for 1 / 4 / 8 workers. The one-source / 65,691-byte / eight-worker serial route parses 15–25 times per first-run batch and 11–23 per repeat batch; the parallel route parses zero. This qualifies repeated prewarmed-source reuse under the synthetic conditions, not cold insertion/eviction or application latency.
- 🟠 [Next] Keep default enablement open. The eight-source / 1,179-byte rotating fixture regresses for multiple workers in both runs (e.g. four workers: 5.003 → 7.761 ms; 9.130 → 11.411 ms). Single-worker repeated-source timings are close, with a small parallel regression in the repeat. Benefit is workload-dependent; do not present the favorable repeated-source ratios as an application-wide speedup. Real workload attribution, stable-host tails and cold/eviction acceptance remain needed.
- 🟢 [Done] Resolve the three Clippy blockers without suppression: remove the unused internal `PaintCache::pixel` method after confirming no callers, use validated fixed-size RGBA chunks in channel clearing, and move production channel code before the test module. Channel behavior and its public signature remain unchanged. All-target renderer Clippy with Skia/heap profiling and warnings denied passes. Historical failure entries above describe earlier snapshots.
- 🟢 [Done] The first full-suite run exposed an old thumbnail test expecting composite channels after importing blue artwork while the selected target remained the hidden base layer. The current selected-target channel contract requires explicit selection of the imported layer; that test now makes the target explicit. No production thumbnail/channel semantics were relaxed. The rerun passes 219 renderer tests, with 47 ignored environment/manual cases.
- 🟠 [Next] Application GUI/input-to-display acceptance, cold insertion/eviction and qualified default policy. Parallel reads remain enabled only with `LUMAPAINT_PARALLEL_TEXT_CACHE=1`. This item does not establish FPS or full Phase 3 completion.

- 🟢 [Done] Final lint-cleanup verification: the full renderer rerun passes 219 tests (47 ignored), all-target Skia/heap-profile Clippy with warnings denied passes again after the thumbnail test update, changed Rust files pass rustfmt checks, and diff whitespace checks pass. The whole-workspace formatting check observed unrelated concurrent shortcut-module formatting differences; this scoped pass does not claim whole-workspace formatting is clean.

## Follow-up: bounds membership changes without an index rebuild (2026-10-09)

- 🟢 [Done] `SpatialIndex::refit` now handles known-to-unknown and unknown-to-known bounds locally. Removal collapses the affected parent, insertion descends by bounding-area growth, and only affected ancestors are refitted. Unknown items remain conservative candidates; invalid viewport queries still return every live item in drawing order.
- 🟢 [Done] Reuse retired node slots and maintain an explicit root, including transitions through an unknown-only index. Repeated toggles of one item do not grow node storage. Persistent node pages preserve previously published snapshots.
- 🟠 [Next] This is bounds-membership maintenance for existing objects, not structural object insertion/deletion in application layers. Structural edits still rebuild their dense-position mappings. A shared `leaves` hash map still copies on its first membership edit; this does not establish constant snapshot-copy cost. Dynamic-tree balancing and end-to-end structural benchmarks remain unfinished.

Validation: the final compiled core test binary passed 310 tests (one existing manual test ignored), including 13 Scene tests and the picking membership regression. Scoped Rust formatting and diff whitespace checks passed. The new core Clippy invocation remained blocked by another Cargo build; it was cancelled without interrupting that unrelated build. Clippy for this patch remains unverified. These tests establish query correctness and node reuse, not application frame-time or Adobe pixel-quality acceptance.
