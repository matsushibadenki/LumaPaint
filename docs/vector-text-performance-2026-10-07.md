# Vector and text performance: measured first changes

## Current specification status (2026-10-10)

This table supersedes earlier chronological remaining-work notes below. The specification is **not complete**; completed subsets must not be reported as complete phases.

| Phase | Current result | Remaining acceptance work |
| --- | --- | --- |
| 1: diagnostics | 🟢 [Done] Opt-in host timings, Rust heap profiling, scoped GPU resources/copies/readback, Skia cache usage, host RSS and correlated display/compute GPU timestamps | 🟠 [Next] Complete worker/mutation coverage, complete live/driver resource memory, Skia-internal GPU timing, cross-platform validation and physical input-to-display measurement; handler-to-present-call diagnostics are implemented |
| 2: full-canvas text composition | 🟢 [Done] Eligible contained canonical text uses independent crops or immutable composite tiles; tiled display no longer materializes a contiguous CPU image | 🟠 [Next] Groups, clips, masks/effects and mixed-content eligibility; compatibility paths still have full canvases |
| 3: retained text | 🟢 [Done] Independent retained GPU crops/local target updates and overlapping tiles; Glyph Atlas masks, shaped instances and opt-in application display pilot | 🟠 [Next] Default-enable quality/GUI gates, fractional zoom/drag, variable/color fonts, decorations and universal transform-only moves |
| 4: native normal zoom | 🟢 [Done] Qualified opaque straight strokes and single pixel-aligned opaque rectangular fills use native GPU rendering at normal zoom; six-zoom pixel comparisons | 🟠 [Next] Cubic fills and other appearance still need edge-quality corrections before normal-zoom routing |
| 5: native strokes | 🟢 [Done] Bounded opaque axis-aligned straight strokes, butt/square caps and dashes; exact rational-conic storage and round undashed straight caps above 150%; retained geometry | 🟠 [Next] Curves/polylines and round joins remain quality-gated; normal-zoom round caps, overlapping/transparent strokes, mixed fill/stroke and general transforms |
| 6: gradients/clipping | 🟢 [Done] Retained GPU linear/radial paints, nested clipping and isolated layer opacity/order; compatibility pixel comparisons pass | Complete within the bounded native geometry contract below; unsupported geometry/appearance retains compatibility rendering. Full Adobe comparisons remain phase 8. |
| 7: incremental Scene | 🟢 [Done] Native transform refits reuse local bounds; idle synchronization skips source walks and retained viewport queries; unchanged layers skip journal copies; structural removals prune cached IDs locally; existing bounds membership changes refit locally; picking retains dynamically balanced stable slots and persistent known-item membership; layer-scoped notifications and coalesced native transform checks; indexed clipping membership removes all-pairs mask lookup; unclipped suffix additions/removals retain prefix bounds and BVH; notified middle edits reuse local bounds and maintain clipped BVHs; adaptive dense rebuilding; retained ID/bounds/membership metadata; one shared journal batch per native revalidation; allocation-free borrowed journal iteration for native synchronization | 🟠 [Next] Extend journal-based reuse to every rendering route; stable native spatial slots, remaining dense metadata scans and full canonical SVG verification; compatibility/mixed-content routes |
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

## Follow-up: dynamic structural picking and persistent membership (2026-10-09)

- 🟢 [Done] Existing-object membership Clippy verification from the preceding entry now passes.
- 🟢 [Done] Stable-ID BVH upsert/removal and height-based local rotations. Sorted insertion of 10,000 entries stays below height 20; removal/reinsertion reuses retired slots. Mixed structural edits match a freshly rebuilt index, including unknown bounds and invalid queries.
- 🟢 [Done] Picking retains stable spatial slots across notified insertion/deletion/reordering/restoration, then maps candidates back into current document drawing order. Unchanged objects retain their spatial leaves. Missing notifications, duplicate IDs, source changes without object notifications and a new Journal instance use the complete rebuild path. Repeated notifications evaluate each existing changed object only once.
- 🟢 [Done] Replace the shared known-item hash map with a compressed persistent binary trie. Its path depth is bounded by machine-word bits, so snapshot membership edits no longer clone the entire known-item map. Sparse and extreme IDs are covered; snapshots preserve removed/updated entries. This changes allocation layout and needs measured validation; it is not a claim of reduced total RSS.
- 🟠 [Next] Structural edits still scan dense document positions; unknown-item lists and retired-slot lists still use shared vectors. Native draw-list/clipping verification has its separate structural rebuild path. Full application capacity, GPU quality and end-to-end timing acceptance remain unfinished.

The spatial example now measures unshared remove/reinsert and shared-snapshot remove/reinsert separately. Shared structural cases use at most eight samples; these are microbenchmarks, not the required 60-second × 5 application runs.

### Persistent membership measurement and list follow-up

Same local macOS Release example, 64 timed samples (eight shared structural samples), 64 warm structural operations. The dynamic-tree/hash-map run and persistent-known-map run are separate sequential runs, not randomized paired application experiments.

| 1,000,000 known entries, operation | Dynamic tree/shared hash map median | Persistent known map median |
| --- | ---: | ---: |
| Unshared remove/reinsert | 5.500 µs | 11.250 µs |
| Shared-snapshot remove/reinsert | 1,457.750 µs | 34.208 µs |

The persistent map reduces first membership-edit snapshot copying but increases unshared-operation cost in this fixture. Query/refit noise also varies between runs. These results do not establish GUI latency, frame deadlines, RSS improvement or a universal speedup.

- 🟢 [Done] Follow-up storage now also uses the persistent trie for unknown-item membership and persistent radix pages for retired slots. This removes whole unknown/free-list copies on a shared spatial snapshot edit; unknown-item enumeration remains proportional to unknown candidates. Trie nodes and free pages have a different allocation layout; memory totals still require measurement.
- 🟠 [Next] Dense document position scans and the native renderer's structural/clipping rebuild paths remain outside this local Scene/picking change.

Final-storage Release recheck (same 64/eight-sample protocol): one-million-entry unshared remove/reinsert median 11.417 µs, shared-snapshot median 35.583 µs. The final core suite passes 317 tests (one existing manual test ignored), and core all-targets Clippy with warnings denied passes. Scoped formatting and whitespace checks pass.

Final integration validation: after restricting structural position scans to object-level changes, the core suite again passes 317 tests (one existing manual test ignored). Layer-order-only notifications now have a zero-scan regression. The renderer's `transform_journal_refits_native_bounds_and_undo_restores_pixels` test passes: transformed candidates, canonical/full-source pixel equality and Undo/Redo restoration are preserved. This is a CPU compatibility raster integration test, not a new explicit native GPU or Adobe comparison.

The final order-only-scan patch also passes core all-targets Clippy (`-D warnings`); its build-lock wait is resolved. No outstanding lint failure is being deferred for this Scene/picking patch.

## Follow-up: layer-scoped journal reads and coalesced native refits (2026-10-09)

- 🟢 [Done] `Journal::read_layer` filters notifications before cloning target IDs. It retains the global cursor and the full journal's missing-range/future-cursor rebuild contract. Global consumers still use `read`; layer-scoped reads still inspect the bounded pending range rather than maintaining a separate per-layer event index.
- 🟢 [Done] Picking skips journal reads when source identity, object count and the layer sequence remain unchanged, even after unrelated layers roll over the journal. A changed source without a matching notification still rebuilds safely. Picking and native verification now use layer-scoped reads when the layer changes.
- 🟢 [Done] Native verification coalesces repeated transform notifications by object position, validating and refitting each final object once. Every notification's geometry/style/visibility/structure/removal flags is checked before any refit; incompatible intervening edits still require complete verification. Both metrics-enabled renderer transform regressions pass, including exact compatibility pixels after Undo/Redo. Core and Skia renderer all-targets Clippy (`-D warnings`) pass.
- 🟢 [Done] Opt-in `scene_journal_read_cloned_events` diagnostics and metrics-enabled core regressions verify 2,000 pending events across 100 layers produce 20 cloned events for one selected layer. Unchanged picking layers clone zero events. The core suite passes 319 tests (one existing manual test ignored).
- 🟠 [Next] Structural native insertion/deletion/clipping reuse and complete journal reuse across all rendering routes remain unfinished. These counts establish reduced notification copying, not application timing, memory totals or FPS.

Final validation for layer-scoped reads/coalescing: metrics-enabled core suite passes 319 tests (one manual test ignored); native module suite passes nine tests (one explicit GPU test ignored), including the two transform-journal tests. Core and Skia renderer all-targets Clippy pass with warnings denied; scoped formatting and diff whitespace checks pass. No new explicit GPU, Release frame-time or Adobe comparison is claimed for this notification-copy change.

## Follow-up: indexed clipping membership during native verification (2026-10-09)

- 🟢 [Done] Replace the per-object full-layer mask search with a borrowed group-name index. Each mask is indexed once, and each object's group path looks up its applicable masks. Output mask indices are sorted in document order and deduplicated, preserving the previous membership semantics for nested groups, repeated group names and multiple masks per group.
- 🟢 [Done] Unmasked layers return empty per-object clip lists after one object scan, without searching any group paths. At 4,096 objects this removes the previous 16,777,216 object/mask-pair checks. This is a work-count reduction, not measured application latency or FPS.
- 🟢 [Done] Add opt-in `native_clip_index_host` timing and object/mask/group-lookup/membership counters. A 512-object nested/repeated/missing-group fixture matches the former full-scan reference, also after reversing document order. A 4,096-object unmasked regression verifies one indexing scan and zero group lookups.
- 🟢 [Done] Metrics-enabled native-module regressions: 11 passed, one explicit GPU test initially ignored. The GPU test was then run explicitly on local macOS and passed, including gradient/clipping comparisons, resident reuse and movement. Arc-mask edge comparisons reported zero alpha difference in the fixture; gradient/clip cases retain their existing comparison thresholds. Existing curved-stroke diagnostic differences are still quality-gated and were not resolved by this metadata change.
- 🟢 [Done] Skia renderer all-targets Clippy (`-D warnings`), scoped formatting and diff whitespace checks pass.
- 🟠 [Next] Structural native changes still perform canonical SVG verification and BVH reconstruction; this removes the quadratic clipping-membership substep rather than completing native structural delta updates. Dense actual mask membership can still require large output lists. Release end-to-end structural timings remain unmeasured.

## Follow-up: bounded native suffix structural reuse (2026-10-09)

- 🟢 [Done] After full native eligibility and canonical SVG equality checks, unclipped additions at the layer end and removals from the layer end retain prefix local bounds, positions and the spatial index. Only appended paths are parsed for new bounds; suffix removals parse no retained paths. Prefix transform notifications are coalesced and refitted locally.
- 🟢 [Done] Structural synchronization invalidates viewport queries but retains derived verification metadata for this revalidation. Removed, hidden or empty affected layers discard old metadata. Missing notifications, journal gaps, changed retained geometry/appearance, middle operations, reordering and clipping/group membership use the full rebuild path.
- 🟢 [Done] Add `bvh_suffix_updates`, `native_suffix_bounds_parses`, `native_suffix_spatial_updates`, retained-object and structural-metadata counters. Thirteen native CPU regressions pass; the initially ignored GPU regression was explicitly run and passes. Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.
- 🟢 [Done] The GPU regression compares retained preparation with a fresh GPU cache after append, delete, Undo and Redo: identical pixels, zero BVH builds, one geometry build per append/restoration and zero per removal, with the prefix buffer retained. CPU tests verify notification rejection is non-mutating and bounds parsing is one for append and zero for removal.
- 🟠 [Next] Canonical SVG generation and eligibility still scan the complete layer (`svg_generations = 1` per structural preparation). General middle insertion/deletion, clipping structural reuse, complete resource budgets and application-scale latency acceptance remain unfinished. These checks establish bounded correctness and avoided rebuild work, not an FPS or application-latency improvement.

## Follow-up: local bounds across middle structural edits (2026-10-09)

- 🟢 [Done] After canonical SVG/eligibility validation, rebuilds reuse surviving local path bounds by object ID across notified insertion/deletion and dense-position changes. Geometry edits and remove/restore cycles reparse the affected path; transforms use the retained local bounds. Duplicate IDs, missing insertion/removal notifications, journal gaps and new journal instances reject reuse before derived data is mutated.
- 🟢 [Done] Add `native_structural_bounds_reused` and `native_structural_bounds_parses` counters. Middle deletion reuses both surviving bounds with zero path parses; Undo reparses only the restored object. The real-GPU regression compares retained and fresh output exactly through middle deletion, Undo and Redo, with geometry builds zero/one/zero respectively.
- 🟢 [Done] Fourteen native CPU regressions pass (one GPU test initially ignored and subsequently run explicitly: one pass). Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.
- 🟠 [Next] Middle edits still rebuild the dense-position BVH and clipping membership, and structural preparation still generates/verifies full canonical SVG. Stable native spatial slots, broader notification reuse, full resource budgets and application-scale timings remain unfinished. No FPS or input-latency improvement is claimed from these operation-count regressions.

## Follow-up: local native BVH updates through middle edits (2026-10-09)

- 🟢 [Done] Canonically validated unclipped structural edits keep the spatial index. Remove retired dense positions and upsert only shifted positions or objects with drawing-bound changes; unaffected positions keep their BVH entries. Add `bvh_structural_updates` and `native_structural_spatial_updates` diagnostics.
- 🟢 [Done] Fifteen native CPU regressions pass; an explicit real-GPU regression passes. Middle delete/Undo/Redo now has zero full BVH builds, matches fresh viewport-query results and produces identical GPU pixels to a fresh cache. Geometry builds remain zero/one/zero for deletion/restoration/deletion.
- 🟠 [Next] Dense positions still require refitting the shifted suffix and rebuilding ID/position metadata. Stable native slots, clipping/group structural updates, full SVG verification cost and application-scale timings remain unfinished. This supersedes the earlier statement that middle edits always rebuild the BVH; no FPS improvement is inferred from the regression counters.

- 🟢 [Done] Final validation for local native BVH maintenance: Skia renderer all-targets Clippy (`-D warnings`), scoped Rust formatting and diff whitespace checks pass.

### Native clipped structural spatial reuse (2026-10-10)

- 🟢 [Done] Canonically validated clipping/group structural edits now retain the native spatial index and local bounds. Recompute clipping membership in current document order; locally refit shifted or changed dense positions. Existing eligibility and canonical SVG checks remain mandatory.
- 🟢 [Done] Sixteen native CPU regressions and one explicit real-GPU regression pass. Clipped child deletion/Undo/Redo performs zero full BVH builds and matches fresh-cache pixels exactly. Derived membership/query tests also cover mask deletion/restoration; a missing mask remains outside native eligibility and uses compatibility rendering.
- 🟢 [Done] Skia renderer all-targets Clippy with warnings denied, scoped Rust formatting and whitespace checks pass.
- 🟠 [Next] Clipping membership is still recomputed across the layer. Stable native slots, shifted-suffix metadata work, full canonical SVG verification and application-scale performance measurements remain unfinished. Phase 7 remains partial; operation counters do not establish an FPS improvement.

### Retained clipping membership for nonstructural edits (2026-10-10)

- 🟢 [Done] Native canonical revalidation retains clipping membership when object IDs/order match and all journal events exclude removal/structure changes. Geometry/style edits update drawing bounds while preserving the membership allocation. Group/mask membership or order changes still rebuild the indexed membership map.
- 🟢 [Done] Seventeen native CPU regressions and one explicit real-GPU regression pass. The geometry-edit regression checks retained allocation and reuse diagnostics; group detachment reindexes. GPU mask geometry editing with a restored clipped child exactly matches fresh-cache output and has zero full BVH builds. Skia renderer all-targets Clippy, scoped formatting and whitespace checks pass.
- 🟠 [Next] Actual membership/order changes still recompute clipping relations; dense-position metadata and canonical SVG validation remain layer-wide. Stable native slots, complete resource budgets and application-scale timing gates remain unfinished. Phase 7 remains partial.

### Retained native ID/position metadata (2026-10-10)

- 🟢 [Done] Canonical revalidation keeps object-ID storage and the ID-to-position map when IDs/order are unchanged, including geometry/style or group-only edits. Changed order rebuilds metadata. Add `native_position_metadata_reuses` and rebuilt-ID counters.
- 🟢 [Done] Seventeen native CPU regressions and one explicit real-GPU regression pass. Mask geometry editing retains ID-array and map-key storage, reports zero rebuilt IDs and matches fresh GPU output exactly. Existing middle deletion/restoration and clipping-membership regressions continue to pass.
- 🟠 [Next] Structural additions/deletions/reordering still reconstruct position metadata and refit shifted dense positions. Stable slots, canonical SVG generation/verification cost and application-scale acceptance remain unfinished. Phase 7 remains partial; no FPS improvement is inferred from these counters.

- 🟢 [Done] Final ID/position metadata validation: Skia renderer all-targets Clippy with warnings denied, scoped Rust formatting and whitespace checks pass.

### Incremental native position-map maintenance (2026-10-10)

- 🟢 [Done] Structural revalidation updates the existing ID-to-position map instead of recreating every string key and map entry. Remove absent IDs, insert new IDs, and rewrite shifted positions; surviving string allocations and map capacity are retained. Add inserted/removed/shifted-ID counters.
- 🟢 [Done] Seventeen native CPU regressions and one explicit real-GPU regression pass. Middle delete/Undo/Redo retains both surviving map-key allocations and updates one inserted/removed ID and one shifted position in the fixture. Rendering, clipping and history match fresh GPU output. Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.
- 🟠 [Next] The ordered ID array, live-ID lookup and dense metadata still perform layer-wide work for structural edits. Stable native slots, full SVG verification cost and application-scale acceptance remain unfinished; phase 7 is partial. These counters establish avoided map/string reconstruction, not a measured application speedup.

### Move retained ordered-ID strings through structural edits (2026-10-10)

- 🟢 [Done] Build the new native drawing-order ID array by moving surviving strings from their previous positions rather than cloning them. Clone only newly inserted IDs; reuse the position map as implemented previously. Add ordered-ID reused/cloned counters.
- 🟢 [Done] Seventeen native CPU regressions and one explicit real-GPU regression pass. Middle delete/Undo/Redo preserves surviving ordered-string allocations; deletion clones zero strings and restoration clones one in the fixture. Fresh GPU comparisons, clipping/history tests, Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.
- 🟠 [Next] Structural edits still allocate the new ordered array and scan the layer, refit shifted dense positions and verify full canonical SVG. Stable slots, remaining memory budgets and end-to-end timing gates remain unfinished; phase 7 is partial. Avoided string clones are not a measured application-speed claim.

### Retain local-bound arrays for unchanged geometry (2026-10-10)

- 🟢 [Done] Canonically validated native edits retain the local-bound array when IDs/order are unchanged and journal events contain no object geometry/removal changes or layer structure changes. Style, transform and group-only edits update derived drawing bounds/membership without allocating/copying a replacement local-bound array. Geometry/structural edits keep the validated update path. Add `native_local_bounds_array_reuses`.
- 🟢 [Done] Seventeen native CPU regressions and one explicit real-GPU regression pass. Group detachment retains the local-bound allocation and values with zero path parses while rebuilding correct clipping membership. Existing shape edits, clipped rendering and Undo/Redo match fresh GPU output. Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.
- 🟠 [Next] Geometry/structural edits still construct bound arrays; dense-slot updates, full canonical SVG verification and end-to-end application timing remain unfinished. Phase 7 remains partial; allocation avoidance is not a measured FPS improvement.

### Share native structural notification reads (2026-10-10)

- 🟢 [Done] Native structural spatial reuse passes its already-read layer notifications into local-bound validation/update. Geometry or insertion/deletion updates no longer clone the same journal batch a second time for bounds processing; independent fallback validation retains its safe read path.
- 🟢 [Done] Seventeen native CPU regressions and one explicit real-GPU regression pass. Middle delete/Undo/Redo asserts cloned events equal exactly one layer-read batch. Fresh GPU comparisons for structural edits, clipping and geometry remain identical. Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.
- 🟠 [Next] Separate refresh/fallback attempts may still read their own batches. Full canonical SVG verification, dense-slot metadata, complete memory budgets and application-scale timing acceptance remain unfinished; phase 7 is partial. Reduced notification copies do not establish measured application latency gains.

### Reuse empty native clipping-membership arrays (2026-10-10)

- 🟢 [Done] Canonically validated layers without masks resize/clear retained clipping-membership storage instead of allocating a replacement outer array. Clearing old rows also removes stale membership after masks disappear. Add `native_empty_clip_membership_reuses`.
- 🟢 [Done] Seventeen native CPU regressions and one explicit real-GPU regression pass. Middle delete/Undo/Redo preserves the membership-array allocation within retained capacity, keeps all rows empty and preserves fresh-index query results. Mask deletion/restoration and fresh GPU output comparisons remain correct.
- 🟠 [Next] Growth beyond retained capacity still allocates. Layer-wide membership checks, stable spatial slots, canonical SVG verification and application timing acceptance remain unfinished; phase 7 is partial. Avoided allocations do not establish measured application speedup.

- 🟢 [Done] Final empty-membership validation: Skia renderer all-targets Clippy with warnings denied, scoped Rust formatting and whitespace checks pass.

## Structural-index host microbenchmark (2026-10-10)

- 🟢 [Done] Add the ignored, explicitly invoked `benchmark_structural_reuse_against_full_index` test: 1,000/4,096 opaque rectangles, deletion at position 1 or the penultimate position, two warm-up and twenty recorded samples. Every sample compares viewport queries with a fresh derived index. No runtime routing threshold was changed.

Command: `cargo test --offline --locked -p lumapaint-renderer --features skia native_bezier::tests::benchmark_structural_reuse_against_full_index -- --ignored --exact --nocapture`.

Local macOS Cargo dev profile (core/renderer optimized by workspace settings), host-only derived-index work. Timed reuse includes journal/bounds validation, dense spatial refits, ordered IDs, map updates and empty membership maintenance. Fresh construction includes path-bound parsing and index/metadata construction. Document deletion/history, full canonical SVG generation/eligibility, GPU preparation and presentation are excluded. Existing cache construction is outside the reuse timer. Retained is measured before fresh each sample; this is a fixed-order preliminary microbenchmark, not randomized comparative acceptance. The reported median is the upper middle sample (index 10 of 20); p95 is nearest-rank index 18.

| Objects | Deleted position | Retained upper median µs | Retained p95 µs | Fresh upper median µs | Fresh p95 µs |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 1 | 616.541 | 719.709 | 870.875 | 1,103.209 |
| 1,000 | 998 | 197.250 | 415.458 | 1,078.292 | 1,854.417 |
| 4,096 | 1 | 2,717.459 | 3,510.000 | 4,081.208 | 5,486.083 |
| 4,096 | 4,094 | 741.792 | 1,082.583 | 3,282.583 | 5,707.750 |

- 🟢 [Done] All four benchmark cases pass exact derived-query comparisons. Scoped formatting and whitespace checks pass.
- 🟠 [Next] Repeat in Release with alternated/randomized ordering and larger sample sets; measure application edits, SVG work, GPU transfers, memory and input-to-display separately. These results do not establish 60/120fps, capacity beyond the application limit or end-to-end latency gains. Phase 7/8 remain partial.

- 🟢 [Done] Structural benchmark validation: Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.

## Release alternating-order comparison and adaptive index maintenance (2026-10-10)

- 🟢 [Done] Alternate reuse/fresh measurement order and calculate the median as the average of the two central samples. Run the actual Skia-enabled Release benchmark. A Skia dependency download/source-sync failure was resolved by packaging the existing cache with exactly matching version 0.153.3 and binary feature key into a temporary local archive, passed via `SKIA_BINARIES_URL=file:///tmp/lumapaint-skia-matching-cache.tar.gz`. The no-Skia attempt ran zero native tests and is not validation.
- 🟢 [Done] The initial Release comparison found early deletion slower for the retained path (1,000: 492.375 vs 418.292 µs; 4,096: 2,088.729 vs 1,754.354 µs). Add an initial adaptive policy: at least 512 remaining objects and more than half the positions requiring updates rebuild the spatial index from retained local bounds rather than performing individual dense refits. Metadata/geometry reuse remains; full BVH builds are counted accurately. This policy is provisional, based on the bounded rectangle fixture.

After the change, five separate benchmark process runs after this task's builds completed; two warm-up and twenty alternating samples per case per run. Each sample checks queries against fresh construction. The table reports the median of the five per-run medians and the range of per-run p95 values, not a pooled p95. These are host-only Release timings; full SVG validation, document/history edits and GPU are excluded as in the earlier benchmark. External system activity is not controlled; this is not a 60-second × 5 application trial or a controlled before/after throughput claim.

| Objects | Deleted position | Adaptive median µs | Fresh median µs | Adaptive trial p95 range µs | Fresh trial p95 range µs |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 1 | 390.272 | 382.709 | 400.583–466.041 | 386.084–1035.459 |
| 1,000 | 998 | 146.896 | 380.188 | 150.166–161.750 | 382.042–413.584 |
| 4,096 | 1 | 1685.958 | 1595.792 | 1722.458–2117.958 | 1583.541–1959.875 |
| 4,096 | 4,094 | 638.104 | 1582.376 | 654.667–713.208 | 1639.250–1852.791 |

- 🟢 [Done] Seventeen native regressions pass (two manual/GPU tests ignored in that invocation); the Release real-GPU regression is explicitly executed and passes. Five Release benchmark runs pass query comparisons. Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.
- 🟠 [Next] Early deletion remains slightly slower than fresh construction (roughly 2–6% in median-of-medians comparisons). Remove residual notification/metadata overhead and broaden the switching-policy fixtures. Stable slots, canonical SVG verification and actual application latency/memory/upload acceptance remain unfinished. The earlier dev result that every tested case was faster is superseded by these Release findings; phases 7/8 are still partial.

## Share validated live IDs through native structural updates (2026-10-10)

- 🟢 [Done] Local-bound validation returns its already-built live-ID set together with bounds. Position-map maintenance borrows that set, removing a second layer-wide hash-set construction and allocation. Validation/rejection precedes derived-data mutation; a named `ValidatedBounds` result keeps the lifetime/data contract explicit. Add `native_live_id_set_reuses`.
- 🟢 [Done] Final Release native regressions: seventeen pass (two manual/GPU tests ignored), followed by an explicit real-GPU comparison pass. Skia renderer all-targets Clippy with warnings denied, scoped formatting and whitespace checks pass.

Five final-code Release benchmark processes after this task's builds/tests completed, each with two warm-up and twenty alternating-order samples per case. Median of the five per-run medians; ranges of per-run p95, not pooled percentiles. Same host-only scope as the previous benchmark: exclude document edits, full SVG verification and GPU. External system activity is not controlled; previous and current runs are separate, not randomized before/after trials.

| Objects | Deleted position | Retained median µs | Fresh median µs | Retained trial p95 range µs | Fresh trial p95 range µs |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 1 | 373.646 | 386.125 | 375.292–390.166 | 383.916–410.542 |
| 1,000 | 998 | 133.791 | 368.958 | 132.666–142.292 | 375.500–393.959 |
| 4,096 | 1 | 1606.042 | 1591.083 | 1653.334–1726.292 | 1632.709–1726.167 |
| 4,096 | 4,094 | 593.416 | 1565.229 | 600.791–693.000 | 1597.250–1770.958 |

- 🟠 [Next] The 4,096-object early deletion case remains approximately 1% slower than fresh construction in these aggregate medians, versus about 6% in the previous separate run. Duplicate set construction is removed, but a definitive speedup or complete regression removal is not claimed. Remaining dense scans/metadata, SVG verification and end-to-end latency/memory/upload acceptance remain unfinished; phases 7/8 stay partial.

## Completed item: single journal batch across native revalidation (2026-10-10)

- 🟢 [Done] Transform refresh, suffix reuse, structural reuse and fallback bounds validation share one immutable journal read, keyed by start cursor, current end cursor, journal instance ID and layer ID. Subsequent attempts reuse the batch without cloning events. New edits/instances/cursors/layers invalidate it, including rebuild/gap results. Successful cursor advancement releases the cached batch; unchanged-layer fast paths still skip reads.
- 🟢 [Done] Eighteen native regressions pass (two manual/GPU tests initially ignored), followed by an explicit real-GPU regression pass. A new unit regression verifies shared batch identity and invalidation after new events. The production GPU preparation regression verifies cloned-event count equals exactly one layer-read batch through geometry fallback/revalidation, with fresh-cache pixel equality. Skia renderer all-targets Clippy, scoped formatting and whitespace checks pass.
- 🟢 [Done] The earlier unfinished native refresh/fallback duplicate-read subset is now closed. This does not close phase 7 or include the separate renderer synchronization read, picking, text workers or every rendering route.
- 🟠 [Next] Full canonical SVG verification, native stable spatial slots/dense scans, other rendering routes and end-to-end application timing/resource acceptance remain unfinished. The prior approximately 1% early-deletion timing difference has not been remeasured for this change and is not declared resolved.

## Completed item: native synchronization without event copies (2026-10-10)

- 🟢 [Done] Add `Journal::read_borrowed`, exposing immutable retained events through a clonable iterator with the same future-cursor/missing-range rebuild safety. Native synchronization borrows that iterator for removals and affected-layer classification rather than cloning every Change/Target into a Vec. Iterator clones copy cursor state only; owned read APIs remain available for asynchronous consumers.
- 🟢 [Done] Five Scene notification tests pass, including a new owned/borrowed equality test over journal rollover, current/future cursors and zero copied-event counters. Eighteen native renderer regressions pass (two manual/GPU tests ignored), followed by an explicit GPU regression verifying actual synchronization consumes events while copying zero, preserves geometry fallback's single batch and matches fresh-cache pixels. Core/Skia renderer all-targets Clippy, scoped formatting and whitespace checks pass.
- 🟢 [Done] Close the separate native synchronization event-copy subset left open in the previous entry. This does not remove event iteration, source-layer checks or cover other rendering/text/picking consumers.
- 🟠 [Next] Other rendering routes, native dense-position metadata, full canonical SVG verification and application-scale timing/resource acceptance remain unfinished. No new timing claim is inferred from zero-copy counters; phase 7 remains partial.

## Completed item: borrowed notifications in picking refresh (2026-10-10)

- 🟢 [Done] Picking refresh borrows Journal events and clones only iterator cursor state. Filtering by layer and object preserves isolation; notification/target strings and the temporary event-reference vector are no longer copied or allocated. Structural validation and stable picking slots retain their existing behavior.
- 🟢 [Done] A metrics-enabled regression interleaves 100 unrelated same-object-ID events with each move/restore, verifying one consumed event, one bounds evaluation, zero cloned events and fresh-index query equality. Journal rollover with a changed source still rebuilds safely. Existing insert/delete/reorder/restore and missing-notification/new-instance regressions pass. Core suite: 327 passed, one existing manual test ignored; all-targets core Clippy passes with warnings denied.
- 🟠 [Next] Notification iteration, structural position scans, other renderer/text routes, native dense slots, canonical SVG verification and application timing remain unfinished. This closes the picking notification-copy subset, not phase 7 or measured input latency.

## Complete borrowed notifications for retained object-image translation (2026-10-10)

- 🟢 [Done] `ObjectCacheState::translated` validates parent/object transform pairs directly from borrowed layer-filtered Journal events. Remove owned notification/target-string copies and the temporary event-reference vector. Preserve consecutive-sequence checks, uniform-translation validation and safe fallback for incomplete pairs, gaps, foreign journals and font/style/geometry changes.
- 🟢 [Done] Five targeted renderer tests pass. Metrics-enabled multilingual text tests at 1,000/4,096 objects verify move/Undo/Redo consumes one pair with zero cloned events, updates only the selected transform and retains text/path storage. New incomplete-pair/future-cursor tests pass. Renderer all-targets Clippy passes with warnings denied; scoped formatting and whitespace checks pass.
- 🟠 [Next] Workspace notification consumption, native canonical validation and dense metadata, other text/render routes and application timing acceptance remain unfinished. Phase 7 remains partial; this allocation reduction is not a measured frame-time improvement.

## Complete copy-free workspace journal availability checks (2026-10-10)

- 🟢 [Done] Workspace preparation checks retained Journal range availability through the borrowed API, without copying notifications/target strings or iterating event payloads. Preserve the original incremental-history/full-raster fallback decision and cursor advancement. Add `workspace_journal_range_checks` diagnostics.
- 🟢 [Done] Seven workspace regressions pass with metrics enabled. Actual dirty-tile preparation checks history once with zero copied-event counters; transparent tile composition stays within the existing one-channel-value tolerance, Undo matches fresh pixels exactly, and an invalid future cursor forces a full raster matching fresh pixels after Redo. Effects, viewport overlap, exterior content, layer order and extreme zoom regressions pass. Renderer all-targets Clippy passes with warnings denied; scoped formatting and whitespace checks pass.
- 🟠 [Next] Retained workspace source comparisons/image copies, native canonical verification/dense metadata, other text/render consumers and application timing acceptance remain unfinished. Phase 7 remains partial; this closes the workspace notification-copy subset without claiming measured FPS gains.

## Complete single raw-image retention for GPU workspace effects (2026-10-10)

- 🟢 [Done] GPU-deferred workspace effects retain only `source_pixels`; the duplicate raw `pixels` buffer stays empty. Full raster preparation removes one full-image copy; dirty tiles update the single retained source buffer. Upload payloads remain owned and unchanged, and CPU effects keep separate raw/adjusted buffers. This removes up to 32 MiB of duplicate retained raw workspace pixels per cache under the existing budget, excluding transient uploads and GPU allocations. Add `workspace_deferred_duplicate_bytes_avoided` diagnostics.
- 🟢 [Done] Seven workspace tests pass. Deferred effect changes preserve raw upload pixels and an empty duplicate buffer; GPU-mode dirty tile preparation matches fresh raw raster within the existing one-value tolerance. CPU mode/effect transitions, Undo/Redo, opacity, exterior content and viewport tests pass. Renderer all-targets Clippy, scoped formatting and whitespace checks pass. Tests exercise host preparation with GPU limits; no new actual GPU timing or frame-rate claim.
- 🟠 [Next] CPU adjusted-image retention/upload copies, source comparisons, native canonical verification/dense metadata and application timing acceptance remain unfinished. Phase 7 remains partial.

## Complete removal of duplicate workspace source snapshots (2026-10-10)

- 🟢 [Done] Workspace order snapshots retain only layer IDs. Idle-cache validation reads source, effective opacity and effects from the existing retained layer instead of maintaining duplicate SVG strings/effect snapshots. Remove one full source-string copy per visible layer per successful preparation; retain exact content comparison and order checks. Add `workspace_snapshot_source_bytes_avoided` diagnostics.
- 🟢 [Done] Eight metrics-enabled workspace regressions pass. The source-byte counter matches the removed snapshot length; a same-length source change invalidates the cache, matches fresh raster pixels exactly and subsequently idles without regeneration. Layer-order, effects, mode switches, tile updates and Undo/Redo tests pass. All-targets renderer Clippy, scoped formatting and whitespace checks pass.
- 🟠 [Next] Full source comparison, retained SvgLayer/object snapshots, CPU adjusted-image/upload copies, native canonical validation/dense metadata and end-to-end timing acceptance remain unfinished. Phase 7 remains partial; no measured latency/FPS gain is claimed.

## Complete retained workspace snapshots for unchanged source (2026-10-10)

- 🟢 [Done] Effect/opacity-only preparation moves the existing retained SvgLayer snapshot instead of cloning it. Preserve its source string and unchanged object storage; update opacity/mask scalars used by future validation. Changed vector-object content is compared exactly and copied when necessary, preserving legacy/missing-notification safety. Source-changing preparation still captures a fresh snapshot. Add retained-source-byte/object-snapshot reuse counters.
- 🟢 [Done] Eight metrics-enabled workspace tests pass. Effect changes verify the same source allocation and reuse counters; opacity changes and Undo/Redo preserve pixels/source revision and retained source allocation. Same-length source changes, CPU/GPU mode changes and dirty tiles retain fresh-raster comparison coverage. All-targets renderer Clippy, scoped formatting and whitespace checks pass.
- 🟠 [Next] Object comparisons and source-changing snapshot copies, CPU adjusted-image/upload copies, native canonical verification/dense metadata and end-to-end timing remain unfinished. Phase 7 remains partial; no measured input latency gain is claimed.

## Complete single raw-image retention for unadjusted CPU workspace display (2026-10-10)

- 🟢 [Done] CPU workspace layers without active effects and with effective opacity exactly 1 retain only their raw source image. Full raster preparation removes the redundant adjusted-image copy and retention (up to 32 MiB per cache under the existing budget). Raw dirty-tile updates likewise skip duplicate adjusted writes. Effect/opacity transitions allocate/resize an adjusted buffer when needed; returning to raw display releases duplicate storage. Owned upload payloads remain unchanged. Add `workspace_cpu_raw_duplicate_bytes_avoided` diagnostics.
- 🟢 [Done] Eight metrics-enabled workspace tests pass. Raw retention is verified empty for the duplicate buffer, source/upload pixels match and the avoided-byte counter equals the image length. Effect changes, opacity, Undo back to raw mode, Redo, partial tile updates, invalid-history rebuilds and fresh-raster comparisons pass. Renderer all-targets Clippy, scoped formatting and whitespace checks pass.
- 🟠 [Next] Active CPU effect/opacity buffers, owned upload copies, source comparisons, native canonical verification/dense metadata and end-to-end timing remain unfinished. Phase 7 remains partial; no measured FPS gain is claimed.

## Complete retained picking IDs during incremental structural updates (2026-10-10)

- 🟢 [Done] Valid incremental structural picking updates build a borrowed ID/position validation map and update the persistent position map in place. Retained IDs are not cloned; deleted IDs are removed and only new IDs are copied into the position/handle maps. Stable slots and pre-mutation duplicate/missing-notification validation remain. Add `picking_structural_id_string_clones` diagnostics.
- 🟢 [Done] Metrics-enabled insertion/delete/reorder/restoration tests verify zero copies for retained-only edits and two string copies per inserted/restored ID. A targeted-notification fixture inserts at the start of 1,000/4,096 objects: both copy exactly two IDs, retain the existing key allocation and return all expected candidates. Core suite: 328 passed, one manual test ignored. All-targets Clippy, scoped formatting and whitespace checks pass.
- 🟠 [Next] Structural full-position validation/scans and the temporary map still scale with object count. Legacy full-layer comparison notifications can exceed the bounded Journal at 4,096 objects and correctly trigger full rebuilding; that route is not optimized by this subset. Missing-range/invalid-notification rebuilds still copy complete IDs. Native slots/canonical verification and end-to-end timing remain unfinished. Phase 7 remains partial.

### Share retained workspace pixels through upload preparation (2026-10-10)

Workspace raw/adjusted images now use `Arc<Vec<u8>>`. Preparation and the renderer's previous-image comparison cache share the selected display allocation, avoiding one whole-image preparation copy and up to 32 MiB of duplicate comparison storage per workspace cache under the existing budget. Standard SVG callers retain the default owned `PreparedSvgLayer<Vec<u8>>` API. No pixels cross the JavaScript IPC boundary.

Partial raster updates use copy-on-write when an earlier prepared/comparison image still owns the buffer; old pixels remain valid for dirty-rectangle detection. Full CPU effect recomputation allocates a replacement when shared instead of copying the previous adjusted image before overwriting it. GPU-effect fallback detaches its output before applying effects/opacity, preserving retained raw pixels. `workspace_upload_bytes_shared` counts payload bytes shared at preparation, not bytes avoided across the entire frame: copy-on-write, rectangle packing and wgpu staging may still copy. This is an allocation/retention improvement, not an end-to-end timing or FPS claim.

Validation: Skia-enabled renderer library suite passed 268 tests with 56 manual/GPU tests ignored by default; all nine workspace regressions passed, including pointer sharing, immutable previous pixels during partial updates, and CPU fallback equality at 50% opacity. The new `gpu_workspace_shared_pixels_preserve_diff_uploads` test passed separately on Apple M4/Metal: effect change, Undo and Redo use the production changed-rectangle/upload functions and match fresh raster bytes exactly after GPU readback. Sandbox execution could not discover a Metal adapter; the same test succeeded with host GPU access. Renderer all-target Clippy with warnings denied, macOS host `cargo check`, architecture boundary validation, scoped rustfmt and whitespace checks passed. Builds used the matching local Skia archive because network dependency download was unavailable. No interactive frame-time benchmark was performed.

### Strided partial uploads and native GUI latency baseline (2026-10-10)

- 🟢 [Done] `upload_svg_rect` borrows the original image from the first changed pixel through the final changed row, passing the original row stride to `Queue::write_texture`. Remove the temporary tightly packed rectangle allocation/copy for narrow updates; full-width updates remain borrowed. Logical upload pixels are unchanged. `svg_upload_packing_bytes_avoided` counts the former narrow-rectangle copy size; the general source-span counter now includes intervening row padding and must not be interpreted as logical GPU payload or physical bus traffic. Copy-on-write of shared workspace images and wgpu's own staging remain.
- 🟢 [Done] CPU rectangle reconstruction/clear/edge tests pass. Apple M4/Metal confirms effects/Undo/Redo through narrow uploads, plus separate exact readback comparisons at 65-pixel source width (260-byte stride), interior rectangles, one-pixel bottom-right, rectangles ending at the bottom/right boundary, and full images.
- 🟢 [Done] Added native fixture generation (`cargo run --offline --locked -p lumapaint-formats --example input_latency_fixture -- /tmp/lumapaint-input-latency-fixtures`) and a log summarizer (`python3 scripts/summarize-input-latency.py LOG`, optional `--warmup N`). Summary tests verify nearest-rank percentiles, warm-up exclusion, unrelated-line exclusion, thresholds and empty-input rejection.

Actual GUI baseline: Cargo dev build (core/renderer opt-level 2), Vite dev UI, `LUMAPAINT_RENDER_METRICS=1`, Apple M4/Metal, 800×574 pixels, scale 1, 78.33% display zoom. Fixtures contain 100/1,000 native 80×80 rectangles on a 100-pixel grid, 32 columns, with off-page objects retained; object count is not visible-object count. Select the vector layer and selection tool. Warm up by dragging the first rectangle from window coordinates (155,235) to (165,235), then capture 12 return/forward pairs (24 native CUA drags), observing AX after every drag. Verify final X/Y/W/H = 8.031/3.528/28.222/28.222 mm so handle resizing is excluded. The earlier small-rectangle pilot mixed resize and move and is excluded from these results. Builds and tests were finished before measurement.

| Objects | Input/present samples | Latest handler p50 ms | p95 ms | p99/max ms | >16.67 ms | Render-host p95 ms | Queue-submit p95 ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 100 | 72 | 1.105 | 16.568 | 16.960 | 2 | 15.645 | 0.581 |
| 1,000 | 72 | 0.883 | 17.159 | 17.533 | 9 | 16.351 | 0.423 |

The input measurement ends at the present API call, excluding OS event queue delay and physical display latency. These are short automated GUI runs, not continuous human/stylus input, sustained 60-second × five Release acceptance, or a before/after speedup. Each input/present sample is a presented group of marked events, not necessarily one mouse event; 83/78 events were marked across these captures. Percentile sample counts are too small for stable tail acceptance. Diagnostic logging and WebView development overhead are included.

All 72 frame snapshots in each capture show zero SVG pixel uploads and zero packing bytes avoided: retained object translation already reuses the GPU image. Consequently this drag baseline does **not** measure the partial-upload optimization's benefit. Cumulative process snapshots were excluded from per-frame totals. Raw input samples, environment, summary and per-frame counts are saved in `docs/input-latency-2026-10-10.json`. Render-host time includes work and waits; a new `surface_acquire_host` timer isolates surface acquisition (including lost/outdated recovery) before attributing the approximately one-frame tail to a cause.

- 🟠 [Next] Shared-image copy-on-write on partial raster edits; sustained Release pointer/stylus, text, pan/zoom and effect-edit captures with repeated matched before/after runs. Preserve FIFO presentation until isolated evidence supports changing scheduling.

Follow-up diagnosis with the final surface timer: another 24 GUI drags / 72 presented samples on the 1,000-object fixture, alternating in reverse order after one excluded warm-up. Final X/Y = 3.528/3.528 mm and W/H unchanged. Latest-handler p50/p95/p99 = **0.948/16.439/16.941 ms**. Frame-host p95 = **15.580 ms**, surface acquisition p95 = **15.062 ms**, queue-submit p95 = **0.575 ms**, present-call p95 = **0.051 ms**. These inclusive stage percentiles are not additive, but the acquisition samples identify substantial waiting inside acquisition rather than the rectangle packing path. Surface acquisition may include presentation pacing and resource availability; this does not measure GPU execution or prove a specific driver cause. FIFO was unchanged. Prioritize main-thread acquisition/frame scheduling analysis and redundant redraw/coalescing before further upload optimization for this drag workload. Raw stage samples are included alongside the input samples in the JSON artifact.

Final verification: macOS application build, renderer/formats all-target Clippy with warnings denied, scoped Rust formatting, architecture boundary and diff whitespace checks pass; CPU upload tests (2), Python summary tests (2), and two explicitly run Metal upload regressions pass. The temporary measurement app and Vite server were stopped after capture. This follow-up does not claim sustained Release acceptance or a measured before/after latency improvement.

### 2026-10-10: display scheduling experiment

Compared the existing immediate mouse-down/up and display-linked drag path with scheduling all pointer phases, on the same 1,000-object fixture, viewport and dev build settings. Each capture followed one forward warmup with 24 alternating 10 px drags and an AX observation after each drag. The baseline yielded 72 input/present groups (latest-handler p50 0.938 ms, p95 16.763 ms); the trial yielded 24 (p50 1.357 ms, p95 50.465 ms). Final selected geometry remained X=8.031, Y=3.528, W=H=28.222 mm; trial Undo restored X=3.528 and Redo restored X=8.031. The trial's final AX capture required reconnecting CUA to the app; its raw logs and final geometry were retained.

Surface-acquisition p95 fell from 15.062 ms to 0.149 ms in the trial, but that did not improve end-to-end handler-to-present timing. There were 72 baseline versus 96 trial render snapshots, and input groups changed, so stage percentile reductions are not acceptance evidence. Deferred terminal rendering also changes which preview/cache preparation paths run before the next interaction; the exact cause of the additional delay needs separate host-stage instrumentation. Rejected the all-phase scheduling trial and restored immediate mouse-down/up. The final change only avoids notifying a display link/AppKit again when a frame is already pending, while still marking every input. No latency improvement is claimed for that final change based on the rejected trial.

`check-display-frame-coalescing.swift` now checks pending-notification suppression, terminal-state preservation and independent views in addition to idle pause. The production pending flag already travels with `window_sessions::Runtime`; its ownership remains unchanged. Fifo presentation remains unchanged. Raw input and render-stage samples for both experimental runs are in `input-latency-2026-10-10.json` under `frame_scheduling_experiment`.

Final pending-notification suppression was rebuilt and independently checked with the same 24-drag procedure: 72 input/present groups, latest-handler p50 0.946 ms, p95 16.784 ms, p99 17.040 ms. Baseline p95 was 16.763 ms; this short run demonstrates no measured latency improvement. Undo/Redo restored the expected X positions, width/height stayed unchanged, and the native canvas screenshot showed the selected translated rectangle. `cargo check`, final app build, scoped rustfmt, architecture-boundary and whitespace checks passed; the real AppKit display-link probe passed. This result is a bounded reduction in redundant scheduling calls, with surface waiting and sustained Release acceptance still unfinished.

### 2026-10-10: input completion and document notification stages

Added opt-in input-ID-correlated `lumapaint-input-stage` records for pointer down/drag/up, release-position presentation and vector commit. Notification records separate memory policy, recovery checkpoint preparation, snapshot notification, workspace notification and shared-view notification; `canvas_redraw` captures synchronous host rendering. Renderer snapshots now separately time preparation before surface acquisition and command construction after acquisition. These are inclusive host durations, not physical display latency, and percentile values must not be added.

The summarizer reads the entire capture before matching stage event IDs to retained presentation ranges, so a pointer-up completion logged after its presentation is retained. Warmup exclusion also excludes stages outside retained event ranges. Three Python regressions cover percentile calculation, invalid captures, stage ordering and coalesced-ID/warmup filtering. Disabled diagnostics perform no stage clock read or log output.

An initial 24-drag run on the same 1,000-object fixture recorded pointer-up p95 61.049 ms while latest-handler-to-present p95 was 16.572 ms. Release-position presentation p95 was 1.411 ms and commit was 0.018 ms. A subsequent run with notification substages isolated pointer-up p95 62.138 ms, total notification 45.746 ms, document snapshot+emit 22.699 ms and workspace snapshot+emit 21.086 ms; memory policy was 0.018 ms and checkpoint preparation 0.344 ms. It identifies synchronous duplicate notification snapshots as a substantial post-presentation main-thread cost. The input/present value for that run was 16.619 ms. Raw stage and presentation samples are retained under `input_stage_diagnosis` in `input-latency-2026-10-10.json`.

The final host notification path creates one workspace snapshot and borrows its active document for `document-changed`. It also serializes the active document once into `serde_json::value::RawValue`, embeds that verified JSON in a borrowed workspace wrapper and uses Tauri's `emit_str_to` for the existing events. Existing names, payload structure and ordering remain; closing the final legacy tab still sends the empty-document notification. Other independent workspace notifications retain their original serialization path. The JSON equivalence regression compares both payloads with ordinary serde serialization, including English/Japanese/Simplified Chinese, quotes, newline/backslash escaping and missing active state.

Snapshot reuse alone produced pointer-up p95 62.902 ms and notification p95 46.906 ms, compared with 62.138/45.746 ms before reuse. With active JSON reuse, the final 24-drag run produced pointer-up p95 69.427 ms, notification 52.355 ms, active/workspace JSON preparation 44.273 ms and latest-handler-to-present 15.487 ms. The workspace-emit substage is now 0.761 ms, but its serialization moved into the JSON preparation substage: that substage reduction is not an overall improvement. Down/drag/release-preview timings also varied across these short captures. No handler-latency gain is accepted from these measurements; sustained, matched Release measurements and reduced notification delivery payloads remain next work. The first combined snapshot stage still includes emit and the reusable JSON work, so its name must not be interpreted as snapshot construction alone.

The final native canvas retained the expected selected rectangle and dimensions after 24 drags; Undo/Redo restored X=3.528/8.031 mm. Saving back at original X and closing the last tab reset the canvas, toolbar and layers. Final app build, wire JSON regression and three summarizer tests passed. All raw intermediate and final samples are retained under `input_stage_diagnosis`; no pixel payload was introduced into the UI bridge.

Final `cargo clippy --offline --locked -p lumapaint --lib -- -D warnings`, scoped rustfmt, architecture-boundary and whitespace checks passed. Temporary measurement apps and the owned Vite server were stopped; the test fixture was saved back at its original X position. Existing unrelated working-tree changes were retained.

### 単一ワークスペース通知の実操作比較（2026-10-10）

`document-changed`と`documents-changed`は同じアクティブ文書を届けていた。UIの既存`updateWorkspace`が文書・タブ・選択を更新するため、内部プロトコルを`documents-changed`だけへ統一。共有ビュー、タイムライン、塗りつぶし設定も購読経路を揃え、最後のタブ終了は`active:null`で初期化する。診断の`document_snapshot_emit`は現在スナップショット準備だけ、`document_workspace_delivery`はTauriのJSON化・配送を含む。区間は包含関係があるため加算しない。

同じ更新済みWebView UI、開発ビルド、1,000図形、1回ウォームアップ後の24ドラッグ。統合前／後の提示サンプルは72／73。操作終了p95 39.048／38.297 ms、文書通知22.498／21.785 ms、最新入力開始→提示16.072／16.029 ms。明確な遅延改善は未確認。前の採取は主に待機中のテスト用コンパイラーと重なり、後はビルド／診断終了後かつ次のテスト開始前に採取した。Releaseでの条件を揃えた比較ではない。生入力・ステージログと制約は`docs/input-latency-2026-10-10.json`の`single_workspace_notification`へ保存。

GUI確認は選択位置・寸法、Undo/Redo、保存、タイムラインのレイヤー表示、最終タブ終了の初期化。今回のGUI採取には共有ウインドウとタイル文書を含まない。

### 既定線設定のスナップショットコピーとJSONを省略（2026-10-10）

`LayerObjectSnapshot`の`strokeStyle`は既定の場合に省略し、UIは既存の`defaultStrokeStyle`へ補完する。Rustの`StrokeStyle`は保持した既定設定と比較し、既定の幅カーブを図形ごとに生成／複製しない。カスタム設定は全項目をコピーして通知する。未設定の`imageFrame`・`fillGradient`・`strokeGradient`もJSONから省略する。文書本体・保存形式・描画設定は同じで、全オブジェクト一覧の差分プロトコルは追加していない。

1,000図形、多言語名付きテストのオブジェクト一覧は677,891→264,891バイト（約61%減）。既定スタイルの複製省略診断は1,000件。旧JSONは省略フィールドへRustの既定スタイルと3個の`null`を補って再構成した。全ワークスペースのサイズや全図形での普遍的な削減率ではない。全14項目の既定判定、カスタム設定・選択ID・Undo/Redo・保存復元を回帰テストで確認。

実アプリは同じ1,000図形文書、Metal／Apple M4、800×574 px、78.33%、Cargo dev／Vite dev、診断有効。前後を交互に2回、各1回のウォームアップ後に24回の短いCUAドラッグ。各ドラッグ後にAX状態を観測し、終了位置を確認。

| p95（ms） | 前・1回目 | 後・1回目 | 前・2回目 | 後・2回目 |
| --- | ---: | ---: | ---: | ---: |
| 操作終了ハンドラー | 62.383 | 29.578 | 73.715 | 35.320 |
| 文書通知 | 48.744 | 22.299 | 64.931 | 18.913 |
| 最新入力開始→提示API | 15.521 | 4.936 | 8.302 | 15.632 |

2回とも文書通知と終了処理は短くなった。ただし提示の方向は揃わず、物理表示の遅延改善や一般的な改善率は主張しない。最初の前の採取はコアテストビルドと、最初の後は後続テスト／Clippyと重なり、再測定は主に待機中のClippyと重なった。アイドル条件を揃えたRelease 60秒×5回の比較ではない。全ログサンプル、条件、制約は計測JSONの`compact_object_snapshot`へ保存。

GUIでは位置・寸法、移動のUndo/Redo、既定の線端／結合／線位置／プロファイル表示、カスタムのマイター制限4→5／Undo 4／Redo 5、復元・保存、レイヤー展開、最終タブ終了を確認。ネイティブ選択メニューのAX操作は再接続要求が繰り返されたため、カスタム設定は数値入力で検証した。展開した一覧のAX出力は500行まで観測し、全件表示数を確認したとはしない。

## オブジェクト一覧の差分通知（2026-10-10）

通常の`documents-changed`は文書メタデータとオブジェクト差分を送る。
差分は前回成功送信の連番を基準とし、変更／追加した行を`upsert`、
構造が変わったレイヤーのID順序を`order`として通知する。
`order`から消えたIDは削除し、順序が不変なら`order`は省略値`null`とする。
行の内容はUI用サマリーだけで、パス形状や画素を送らない。
移動で一覧のサマリーが不変でも、選択範囲などの文書メタデータは更新する。

購読開始・文書切替・連番不一致では全件通知を使う。
初期スナップショット取得と購読を並行しないため、古い初期値の上書きを避ける。
通知ミラーはRust文書の代わりになる状態ではなく、送信差分の基準である。
各ウインドウのアクティブ文書のみ保持し、4,096件を超える文書は保持せず全件通知を使う。
コアで全件サマリーを生成する作業と文字情報のコピーは引き続き残る。

Release比較の全件モードは`LUMAPAINT_WORKSPACE_FULL_NOTIFICATIONS=1`で指定する。
同じ最適化済みバイナリの通知方式だけを切り替えるため、過去のコミット全体との比較ではない。
ビルドは`cargo build -p lumapaint --release --features tauri/custom-protocol --offline --locked`で本番用UIを埋め込む。
単独の`cargo build --release`はこの構成では開発用URLを参照するため、計測には使用しない。
`LUMAPAINT_RENDER_METRICS=1`によるホスト計測を両モードで有効にする。
物理入力／OSキュー／画面発光の遅延は計測範囲に含めない。

### Release継続比較の結果

Apple M4／Metal、800×574 px、倍率78.33%、1,000オブジェクトの同一文書を使用。
本番用プロトコルの埋め込みUIと同じReleaseバイナリで、全件／差分を交互に切り替えた。
ビルド・テスト・Clippyは計測開始前に完了し、両方式で同じ診断を有効にした。
各方式5回、各試行60秒以上（30秒以上の操作区間を2つ、区間間のツール待ちは操作時間から除外）。
全件410ドラッグ／1,232提示サンプル、差分416ドラッグ／1,250提示サンプル。
各プロセス開始時の1ドラッグと後続試行前の往復をウォームアップとして除外した。
6プロセスの交互グループで実施しており、「各方式5プロセス」ではない。

値は**各試行のp95の中央値**であり、全サンプルをまとめたp95ではない。
区間は包含関係があるため加算しない。

| 計測区間 | 全件通知 | 差分通知 |
| --- | ---: | ---: |
| 通知送信処理 | 0.418 ms | 0.326 ms |
| 文書スナップショット生成 | 0.572 ms | 0.576 ms |
| 文書通知全体 | 1.385 ms | 1.324 ms |
| 操作終了処理 | 18.313 ms | 18.175 ms |
| キャンバス再描画 | 16.204 ms | 16.159 ms |
| ハンドラ開始→提示API（最新入力） | 16.333 ms | 16.263 ms |

通知1件の平均バイト数を各試行で求めた中央値は246,703→2,812バイト（98.86%減）。
診断のプロセス合計は累積値なので、各キャプチャの最終値から初期値を引いている。
未公開の境界作業は含まれず、操作件数そのものとは一致しない。
全件／差分の診断件数でも、指定した通知方式が使用されたことを確認した。

送信処理p95の中央値は約22%短縮した。
操作終了と提示の差は小さく、提示p95の試行範囲は全件16.262〜16.340 ms、
差分16.192〜16.359 msと重なるため、表示遅延の明確な改善とは判断しない。
コアのサマリー再生成も短縮していない。
キャンバス再描画が支配的な区間として残っているため、次は描画先取得待ちと描画予約を優先する。

`docs/input-latency-2026-10-10.json`の`release_object_delta`に
ビルド／文書のSHA256、実施順序、各試行の操作時間・件数、全入力・ステージの生サンプル、
p50／p95／p99／最大値と公開カウンタ差分を保存した。
CUAによる短いマウスドラッグとAX観測の比較であり、人の連続した筆入力ではない。
文字・パン／ズーム・効果編集、物理入力から画面発光までの計測はこの結果に含めない。

## 描画先取得待ちの入力単位計測（2026-10-10）

`LUMAPAINT_RENDER_METRICS=1`で、`render_prepare`、`surface_acquire`、
`render_encode`、`queue_submit`、`queue_present`を既存の入力ステージ形式で記録する。
レンダラーが保持する最新入力マーカーを値として捕捉するため、AppKitの予約描画でも入力IDと対応する。
入力ハンドラ開始から区間終了までの経過と、区間そのもののホスト時間を分ける。
GPU実行時間、物理入力、画面発光時間には換算しない。
取得失敗／復旧の試行も区間に含まれ、同じ入力IDが後で提示された場合に集計される。
ステージは包含関係があるため加算しない。

FIFOを保ったフレーム保持数の診断比較は
`LUMAPAINT_RENDER_METRICS=1 LUMAPAINT_SURFACE_FRAME_LATENCY=1`または`2`で実行する。
診断を有効にしない実行では、この環境変数を設定しても挙動を変更しない。
無効値は既定値を保つ。起動ログで指定した値を確認する。
比較用Releaseのビルド方法は前節と同じ本番用プロトコルを使う。

### FIFO保持数の比較と採用判断

保持数2／1／1／2の順で、各24ドラッグを同じReleaseバイナリで計測した。
提示p95は2で17.071／16.252 ms、1で33.710／32.984 ms。
終了ハンドラーp95も2の19.259／18.185 msから1の35.738／35.977 msへ増えた。
1では終了プレビュー区間が約17.7 msとなり、その後の描画先取得でも約16.5 ms待った。
保持数を減らす案は不採用とし、通常実行のFifo・保持数2を維持する。

この試行では直接呼び出した終了プレビューに提示マーカーが付かない既存の欠落があった。
終了ハンドラーと`vector_release_preview`区間には含むが、提示とGPUステージ集計では対象外。
`surface_frame_latency_experiment`に全試行・ビルド識別・生サンプルとこの制約を保存した。
後述の改善版では描画関数内でマーカーを捕捉して、この欠落を修正した。
過去の計測と改善版の提示サンプルを直接比較しない。

### 最終位置のフレームを再利用

通常のベクター移動では、終了位置のプレビュー提示後に文書を確定し、
同じ表示を直後の再描画で提示する経路があった。
提示APIに到達したことをレンダラーの連番で確認できる場合だけ、
確定後の文書ID・リビジョン・キャンバストークンを記録し、重複再描画を1回省く。
取得失敗では連番が進まず、省略しない。版や文書が変わればフラグを破棄する。
SVGワーカーの予約と準備、確定・通知は行い、後続の正規描画を継続する。
複製・文字リサイズには適用しない。GPU上の提示済みフレームを保持し、画素コピーは追加しない。

同じReleaseバイナリの比較用に、診断時のみ
`LUMAPAINT_REDUNDANT_RELEASE_DRAW=1`で従来の重複再描画を復元する。
通常実行では削減を有効にする。保持数は両方式とも2である。
改善版は直接の終了プレビューにも入力マーカーを付け、
省略する可能性がある再描画の入り口ではマーカーを付けない。

1,000図形・800×574 px・78.33%・Apple M4／Metalで、
従来／改善／改善／従来の順に各24ドラッグ（計96回）を実施した。
改善の2試行は同じプロセスを継続し、前後の従来試行はそれぞれ新規起動した。
各起動時の最初のドラッグを除外し、コンパイル・テスト・Clippyは計測前に完了した。
各試行の操作時間は約18秒で、60秒×5回の受け入れ試験ではない。

| p95の区間 | 従来1回目／2回目 | 改善1回目／2回目 |
| --- | ---: | ---: |
| 最新ハンドラー開始→提示API | 16.262／16.394 ms | 0.699／0.799 ms |
| 操作終了ハンドラー | 18.209／18.585 ms | 2.882／2.653 ms |
| 描画先取得 | 15.021／15.078 ms | 0.049／0.051 ms |

提示サンプル数は従来72／72、改善48／49となり、母集団が変わる。
削除した遅い重複提示だけの効果と区別するため、各試行24件の終了ハンドラーも確認した。
終了処理の待ちも短縮し、従来経路への復帰で約18 msへ戻ることを確認して採用した。
Undo／RedoでX=3.528／8.031 mm、寸法28.222 mmと選択枠の位置を確認。
測定用文書は復元・保存し、元ファイルとSHA256が一致した。

`release_frame_reuse`にSHA256・実施順序・時間・全入力／ステージの生サンプルと集計を保存した。
診断カウンターの公開値は累積・間欠的であり、その範囲を正確なドラッグ件数としない。
物理入力／OSキュー／画面発光は計測対象外である。
60秒×5回、文字移動・複数選択・倍率／効果・ワーカー競合での検証は次の工程に残す。

## 重複提示削減の継続比較（2026-10-11）

Apple M4／Metal、1,000図形、800×574 px、78.33%、Fifo保持数2で計測した。
`cargo build -p lumapaint --release --features tauri/custom-protocol --offline --locked`
で生成した同じバイナリを使用し、両方式で`LUMAPAINT_RENDER_METRICS=1`を有効にする。
従来方式のみ`LUMAPAINT_REDUNDANT_RELEASE_DRAW=1`を設定する。
所有するビルド・テスト・Clippyは採取開始前に完了した。他アプリは起動しており、専用の隔離環境ではない。

各方式5回、各試行60秒以上。30秒以上の操作区間を2つ採り、区間間のツール待ちは操作時間から除外した。
実施順序は従来1→改善1・2→従来2・3→改善3・4→従来4・5→改善5の6プロセスグループ。
各方式5つの新規プロセスではない。起動時の1ドラッグと同一プロセス内の次試行前の往復を除外した。
改善1では自動復旧の表示変化で初回採取が中断したため、位置を再確認して採り直した。
中断区間34ドラッグは比較へ含めず、改善1に追加のウォームアップがあることを記録した。

両方式とも計410ドラッグ。終了ハンドラーは各410件、提示サンプルは従来1,230／改善820件。
各操作後にAXを観測し、各区間末のX=8.031 mm、Y=3.528 mmと寸法を確認した。
プロセス終了前に最後の移動をUndoしてX=3.528 mmへ復元し、保存した。

値は各試行p95の中央値（括弧内は5試行の最小〜最大）であり、全サンプルをまとめたp95ではない。
各区間は包含関係があるため加算しない。

| 計測区間 | 従来経路 | 重複提示を削減 |
| --- | ---: | ---: |
| 終了ハンドラー | 17.986（17.224〜18.597）ms | 6.183（4.098〜7.220）ms |
| 最新ハンドラー開始→提示API | 14.183（2.598〜16.149）ms | 1.449（1.074〜1.670）ms |
| 描画先取得 | 11.590（0.087〜14.906）ms | 0.096（0.088〜0.116）ms |
| キャンバス再描画 | 15.048（4.568〜15.984）ms | 4.298（3.252〜5.662）ms |
| 最終位置プレビュー | 2.801（1.107〜2.998）ms | 2.946（2.073〜3.489）ms |
| 文書通知全体 | 2.977（1.265〜3.282）ms | 3.059（1.790〜3.733）ms |

終了処理p95の中央値は約65.6%短縮し、試行範囲も重ならなかった。
提示サンプルの母集団は重複提示の削減で変わるため、そのp95だけを改善の根拠にしない。
最終プレビューは維持し、終了ハンドラーの待ちが短くなることを今回の採用根拠とする。
文書通知とプレビュー自体の短縮は確認していない。物理的な表示遅延・FPSには換算しない。

### 複数選択と三言語文字の表示検証

生成例は従来の100／1,000図形に加え、`latency-multiple.lumapaint`と`latency-text.lumapaint`を生成する。
`cargo run -p lumapaint-formats --example input_latency_fixture --offline --locked -- /tmp/lumapaint-input-latency-fixtures`
で再生成できる。GUIでは対象のベクターレイヤーを選び、二図形はShiftクリックで選択する。

二つの図形を同時に10画面px移動し、X=3.528→8.031 mm、Y=3.528 mm、W=63.5 mm、H=28.222 mmを確認した。
選択枠と二図形が追従し、Undo／Redoで位置を復元することを目視とAXで確認した。

文字は`English office`、`日本語の文字`、`简体中文文本`の三つを個別に移動した。
X=14.111→18.615 mm、Y=28.222／70.556／112.889 mm、W=176.389 mm、H=17.78 mmを確認した。
字形と文字枠が同時に移動し、寸法を保ち、Undo／Redoでも一致した。
全検証文書を復元・保存し、生成直後とSHA256が一致した。検証用アプリは終了した。

これは表示・状態更新の確認であり、文字・複数選択の継続遅延比較やIME／編集組版の検証ではない。
異なる倍率／効果、SVGワーカー競合、物理入力、実ペン、他OSは残る検証範囲である。

`input-latency-2026-10-11.json`にバイナリ・文書SHA256、実施順序、操作時間、全入力／ステージの生サンプル、
中断区間、各試行と集約値、GUIの位置・寸法を保存した。
10試行すべての生サンプルから集計を再現し、終了ステージ数が操作件数と一致することを検証した。
入力マーカーが次の入力群へ持ち越される場合があるため、最初／最新入力の値は区別して保持する。


## SVGワーカーの借用判定とRelease表示確認（2026-10-11）

予約済みまたは失敗済みの同じRasterKeyを、SVGレイヤーの複製前に除外する。
共有GPU描画が先に準備できた場合は状態を解除し、文書・版・キャンバス変更は新しい要求として扱う。
ワーカー完了時は必要なレイヤーIDだけを借用し、ID文字列のみを所有する。
予約／フレーム保持の準備完了判定も共通化した。ネイティブ描画の準備済みIDだけでは古い版を誤認するため、
Journal識別子と文書リビジョンの一致、効果がないことも確認する。

埋め込みUIのRelease（Apple M4／Metal、Fifo保持数2）で、1,000図形を50%と200%表示で確認した。
10画面pxの移動で、50%ではX=3.528→10.583 mm、200%ではX=109.361→111.125 mmとなり、
寸法を保ち、Undo／Redoで位置を復元した。
効果付き5,000図形も200%で同じ移動と履歴を確認し、露光量1→0.5で青色の明るさが変わることを目視した。
露光量と位置を戻して保存し、生成直後の文書SHA256と一致した。

累積診断ではIDのみの完了判定8件で11,107,864 bytesのSVGソースコピーを回避した。
これはソース文字列のバイト数であり、オブジェクト一覧の容量やGPU転送量を含まない。
ワーカーへ実際に渡すスナップショット12件・ソース12,691,796 bytesは引き続き必要だった。
待機中の重複除外カウンターは今回のGUI操作では観測できず、状態遷移の単体テストで検証した。
今回の操作は表示とコピー削減の診断で、対照群を揃えた入力遅延比較ではない。

アプリ178テスト（手動1件除外）と実GPUの準備完了拒否テストが通過した。
GPUテストは古い文書リビジョン、別Journalの描画用コピー、効果付きレイヤーを対象にした。
Clippy、非Skia構成のcheck、Releaseビルドも通過した。
検証条件・文書／バイナリSHA256・累積カウンターの全17スナップショットを
[検証JSON](render-worker-validation-2026-10-11.json)に保存した。
ワーカー待機中の急な倍率変更や競合は未確認で、次の検証対象として残す。


### ネイティブ準備状態とViewportの一致

ワーカー処理の前後では、直前の描画が同じ文書・版でも別の倍率で行われている場合がある。
ネイティブ描画の品質条件と表示対象の一覧はViewportに依存するため、準備した表示変換を固定長のキーで保持する。
現在の幅・高さ・スケール・倍率・パン・文書寸法と一致する場合に限り準備済みとして扱う。
文書用のRasterKeyは変更せず、倍率が変わっても文書座標のCPUビットマップを利用できる設計を維持する。
実GPUテストでは200%の準備状態が50%・パン・幅・スケール変更に流用されないことを確認した。
この変更は準備判定の整合性を補強するもので、入力遅延の改善値はまだ測定していない。


更新した同じReleaseで1,000図形の50%→200%切り替えを3回行い、200%の移動で
X=109.361→111.125 mm、Undoで109.361 mmを確認した。
効果付き5,000図形では200%から露光量を変更し50%へ切り替え、
X=3.528→10.583 mmの移動・Undo／Redoを確認し、200%へ戻した。
露光量の途中入力は0.51となったため、0.5へ確定し、最後に1へ復元した。
図形・色・選択枠を返却されたスクリーンショットで確認し、保存後の全文書SHA256は元と一致した。
この実操作ではワーカー待機と操作の重なりを立証できていない。
競合完了の決定的な検証と、条件を揃えた入力遅延比較は未完了である。
検証JSONの`viewport_readiness_followup`に別バイナリのSHA256・全21カウンタースナップショット・操作条件を記録した。
