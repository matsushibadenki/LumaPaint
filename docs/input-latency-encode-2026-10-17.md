# Workspace preparation and native input — 2026-10-17

The previous `render_encode` metric covered host preparation as well as command encoding. It is now split into setup, native geometry, motion mesh, glyphs, layer images, workspace images, brushes, timestamp collection and render passes. Stages are inclusive host durations, not GPU execution times; nested stages must not be added together.

An instrumented Release diagnostic on the original 600-shape/text/mask/blur fixture, frame 15, 800×366 Metal viewport, backing scale 1, collected 30 native magnifier presentations after warmup. Handler-to-present p50/p95 were 26.918/33.600ms; encode p95 was 31.392ms, including workspace preparation 30.431ms. Render-pass recording p95 was only 0.031ms. The original ~21ms aggregate therefore identified a host image-preparation bottleneck, not a ~21ms GPU command-recording cost. SVG parsing repeated 90 times in those 30 inputs (median 2.145ms, maximum 9.100ms per parse). Metal readback occurred 30 times (median 0.623ms, maximum 10.543ms); it was an additional cost, not the whole explanation.

Workspace images now use a separate bounded parsed-tree cache (32 entries / 32MiB accounted estimate, not RSS), retaining eligible shape, mask and Gaussian-blur trees independently of viewport zoom/pan. Keys require full source equality; existing shared-cache nonblocking lookup, LRU eviction, retirement outside locks and source/entry bounds are reused. Decoded images, gradients, patterns and other filter kinds bypass retention. Primitive overhead, geometry, mask/subroots and glyph layout are charged; the immutable shared font database is not charged repeatedly. In-flight Arc users may outlive eviction. This is additional to the existing text-tree and scene-pixel caches.

CPU-owned workspace rasters use resvg directly, avoiding a Metal draw/readback round trip before host composition and GPU upload. Fit, viewport transform, exterior pixels, premultiplied alpha and resource policy remain the same. The normal document/object backend selection remains; workspace images (including the high-zoom/effect replacement path) use the compatibility renderer and can differ from Skia antialiasing. Complete backend pixel parity is not asserted. `LUMAPAINT_WORKSPACE_GPU_BASELINE=1` restores the old parsing/backend route for diagnostics.

`LUMAPAINT_ONION_METRICS=1` additionally records main-thread source snapshot, worker queue wait, main-thread callback wait and accepted texture upload, keyed by generation. Discarded pending jobs have no worker sample; stale callbacks can have a main-queue sample but cannot upload. Upload timing includes host resource registration/API calls, not GPU completion or presentation. These asynchronous job stages are not total seek-to-display latency.

## Verified scope and remaining acceptance

Implementation, the 30-input diagnostic, workspace/parse/native regressions and the explicit real-GPU diff-upload test are complete. Release build, formatting, architecture/helper checks and Clippy with warnings denied passed. **The before/after 60-second × five mixed-input comparison did not complete and no optimized latency gain is claimed.** See [numeric records and status](input-latency-encode-2026-10-17.json).

In the accepted baseline diagnostic, 12 reference jobs exposed these host costs:

| Stage | Median | Maximum |
| --- | --- | --- |
| Main-thread document snapshot | 0.016ms | 0.058ms |
| Worker queue | 0.0165ms | 2.845ms |
| Main-thread callback queue | 1.6645ms | 17.192ms |
| Accepted host texture registration/upload | 0.3635ms | 0.835ms |

This is a small warm-zoom sample, not a production-art acceptance distribution. The callback queue can overlap a synchronous frame, and upload does not measure GPU completion. The source snapshot remained much smaller than the workspace-preparation cost in this diagnostic.

The restricted GPU test initially failed because no Metal adapter was exposed. The same test passed with normal access. Restricted Clippy processes remained waiting without CPU activity; they were temporarily suspended to isolate planned timing, resumed, then terminated before rerunning the same checks with normal access and a single job. That final Clippy run passed. No validation process remains intentionally suspended.

## Planned mixed-input comparison protocol (pending)

The mixed artwork fixture adds `docs/images/illust.svg` (Illustrator-authored, 405 paths and 11 polygons) to the existing synthetic 600 rectangles, three-language text, SVG mask/blur and two pixel tracks/six exposures. Reproduce with:

```sh
cargo run -p lumapaint-formats --example onion_latency_fixture -- /tmp/encode-artwork.lumapaint docs/images/illust.svg
```

This includes real artwork from the repository but is a constructed validation project, not a supplied production `.lumapaint` project. The user was asked for a production-project path; that broader acceptance must remain separate if no project is provided.

The app is an embedded-UI optimized Release, running in the same owned QA identity. The accepted baseline diagnostic used a tiny launcher to set `LUMAPAINT_RENDER_METRICS=1` and `LUMAPAINT_ONION_METRICS=1`, redirect stderr, and exec the native executable. Later recovery attempts used the normal executable with a QA-only constructor library to set the same flags/redirect stderr, then aligned bundle display/executable names; these did not establish a completed acceptance run. All UI interaction is actual CUA native input; no Rust-side synthetic event injection is used. Build/test activity must finish before timing. Binary/source/fixture hashes and per-trial numeric events/stages are retained in the accompanying JSON evidence.

The intended protocol (functionally exercised for one baseline round, not completed as sustained acceptance) seeks frame 1, edits the existing root cel, adds one brush stroke, undoes it, seeks frame 15, then performs five native zoom-in clicks and five zoom-out clicks at window (492,343). State checks require 1→2→1 root strokes, frames 1→15 and 25.44%→77.64%→25.44%. The base view is fit (49.6875%) followed by three native reductions, with centered pan; viewport is 800×366 pixels at scale 1 in a 1,200×800 window. Onion skin remains enabled in Scene mode, before/after two each. Each round changes document revisions and exercises real reference regeneration/worker contention.

The input metric remains native handler start→present API call. It excludes the OS event queue, GPU completion and physical display. Frame changes are included in the workload, while asynchronous reference stages are reported separately; they are not presented as complete UI seek latency. Native automation pace and state observations limit event rate and differ from a continuous hardware stylus trace. Tool launch/rebinding, dialog handling, setup and compilation are excluded.


## UI automation blocker

Native coordinate/screenshot calls repeatedly failed with `noWindowsAvailable`. During other interrupted periods, surface acquisition returned before encode/present (consistent with the renderer's occluded/timeout handling), leaving stale native imagery while WebView controls changed. These intervals cannot be included in input-to-present statistics. Bundle rebinding, window raise, zoom/restore, runtime reset, changing the owned bundle from an exec launcher to its normal executable and aligning display names recovered some native input/present pairs but did not stabilize mixed drawing/seeking operations. Attempting to select Dock timed out in the automation service; this tool delay is not application latency. No continuous trial met acceptance, and failed/setup intervals were excluded.

The owned fixtures are still reproducible, the measurement hooks and normal Release are available, and the original artwork/fixture files were not overwritten by UI testing. The QA application was closed after discarding only its owned in-memory test edits. A supplied production `.lumapaint` project and a stable native input/capture target are required to finish the remaining acceptance. Full Skia/resvg antialiasing/effect parity, Windows/Linux and physical display latency remain outside the verified scope.
