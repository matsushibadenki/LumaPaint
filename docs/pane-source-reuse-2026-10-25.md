# Pane source reuse and drawing latency investigation — 2026-10-25

## Changes

Standard inactive-pane synchronization now reads canonical revision, dimensions, visible-layer count and canvas color through a synchronous borrow. It updates frame/visibility/masks and checks the existing full-viewport/revision/presentation reuse conditions **before** creating a rendering fork. Unchanged panes avoid both `clone_for_rendering` and the full `DocumentSnapshot` previously created to read canvas color. Invalid/missing document IDs still fail, hidden/recreated/changed/failed surfaces still render, and tiled content keeps its existing path.

Shared standard content also has a synchronous borrowed accessor. Background animation ticks use that accessor rather than cloning a shared document before evaluating its current frame. Canonical document/history ownership does not move; these callbacks must not re-enter session/document stores. Animation evaluation can still create its required evaluated preview. The initial version retained rendering forks for changed panes; the synchronous borrowed-render follow-up below removes that remaining unconditional standard-pane fork. This is not a blanket zero-copy renderer.

`LUMAPAINT_PANE_SOURCE_REUSE=0` restores eager source construction and full metadata snapshots for a same-binary control. The earlier retained-GPU presentation and conservative input policy remain enabled in both modes. Production defaults to source reuse enabled. Counters distinguish real standard render forks, full metadata snapshots and forks avoided; the control does not report an avoided fork merely because GPU presentation was reused.

## What the previous Release captures show

The four completed final-policy captures are analyzed by `scripts/analyze-input-stages.py`; [the stage JSON](drawing-latency-stages-2026-10-25.json) retains per-run scopes/counts. The first two gestures in each phase are excluded, matching the earlier comparison. Only stages whose input IDs match retained latest presentations enter the breakdown. Stage durations overlap, and neither their p95 values nor their medians should be added.

- Pointer movement handler host p95: about 0.013–0.018ms. Native render preparation p95: about 0.0013–0.0021ms. Cached layer encoding p95: about 0.0046–0.0063ms. These are CPU stage timings after warmup, not GPU completion times.
- Handler start→renderer preparation start p95: about 1.88–2.59ms. That interval includes handler work and native display scheduling; it is not all rendering work. The new exact display-link checkpoint distinguishes it from synchronous redraw entry.
- Surface acquisition p95 varies 0.027–0.751ms, with one 12.709ms maximum. Acquisition can contribute individual outliers, but it does not explain every sample or the much larger source→handler phase differences.
- Pointer-up duration p95: about 7.23–13.49ms, with only 14 retained release events per phase. This is a different, small sample cohort from the hundreds of movement presentations; its p95 is effectively a maximum. Stroke finalization/history/notifications remain relevant release-cost work.
- Source timestamp→pointer probe median varied 8.75–19.32ms across drawing runs. These are app-generated source timestamps and include producer scheduling, Tauri main-thread dispatch, AppKit queue and pre-pointer work. They are not physical hardware timestamps or a pure OS queue measurement. This variation limits the earlier combined-latency comparison.

The evidence prioritizes avoiding redundant completion/notification work and investigating event/display scheduling. It does not establish that GPU execution is the primary cause, nor that eliminating source snapshots alone will reduce steady movement p95.

## New timing probes

`native_display_callback` captures a pending latest marker at the actual display-link callback. `native_redraw_entry` separately marks the common redraw entry, including synchronous paths. Neither consumes the marker or logs when performance tracking is off. Existing renderer preparation/acquisition/encoding/submission stages remain available.

QA-only `LUMAPAINT_QA_DISPATCH_TIMING=1` logs synthetic source time and the main-thread dispatch time immediately before AppKit posting. Matching with the early pointer probe separates producer/main-dispatch/factory wait from post/logging/AppKit/pre-pointer wait. The timestamps share system uptime and accept the known 1ns factory conversion difference. This is opt-in synthetic transport instrumentation and cannot be interpreted as physical input-to-photon latency.

## Validation

Rust regression includes borrowing the actual active/inactive canonical document address, unchanged revision/undo history, unknown-ID rejection, and access from a shared-view neighboring document without switching the active document. The stage parser checks warmup/unrelated-event exclusion, 1ns conversion matching and the interval calculations. Broader source-reuse performance results require the new Release comparison and are recorded separately from the old-policy evidence.

## Final validation and deferred native comparison

🟢 [Done] Rust library 223 passed / 2 ignored. The final QA-feature optimized Release and all-target Clippy with warnings denied succeeded. The stage parser has three tests (including ambiguous last-move/up timestamp exclusion), the existing latency parser has five, and layout/format/core-boundary checks pass. Canonical-address/history/unknown-ID/shared-neighbor regression passed. Source/binary hashes and check results are in [the evidence JSON](pane-source-reuse-2026-10-25.json).

⭕️ [Pending] The user explicitly requested that the native comparison be performed later. Two control attempts stopped at the native safety guard before any complete accepted phase. One completed two warmup gestures; the retry did not complete a gesture. The guard checks foreground state, document identity and run ownership, but the exact failing predicate was not logged, so its precise cause remains unverified. Neither attempt is accepted as a performance result. No new source-reuse latency/FPS gain is claimed, and the old-policy stage values above are not results for this new binary.

The dedicated QA apps exited normally after discarding temporary changes. All nine owned fixture hashes are unchanged. Full runtime logs were compressed and the duplicate temporary QA bundle removed. Do not automatically resume native input after the user's deferral.

For later reproduction, use the same QA build/fixtures/layout and input protocol as [the earlier complete comparison](continuous-input-release-2026-10-25.md), keeping `LUMAPAINT_PANE_INPUT_PRIORITY=1` and `LUMAPAINT_PANE_CONTINUOUS_PRIORITY=1` in both modes. Switch only `LUMAPAINT_PANE_SOURCE_REUSE=0/1/1/0`; enable `LUMAPAINT_QA_DISPATCH_TIMING=1` consistently. Reuse `scripts/summarize-continuous-input.py` and feed accepted phase metadata to `scripts/analyze-input-stages.py`. Reject incomplete phases, and separate all-phase fork counters from latency samples after the two warmup gestures. Last move/up share a source timestamp and are excluded only from source→dispatch decomposition if ambiguous, with the missing-match count retained; their presentation/handler timings remain in the overall comparison. Stage distributions and synthetic dispatch/probe intervals must not be added as independent components.

## Synchronous changed-pane follow-up — 2026-10-25

Standard panes that need a new presentation now also render through the canonical synchronous borrow. `render_standard_pane` shares animation-preview ownership rules between the borrowed production path and eager-source diagnostic control. A retained animated preview continues to own the surface even if its acquisition is deferred; the canonical editing picture must not replace it. No borrowed document reference escapes the call or moves into a worker. Renderer resources/caches retain their own data, and animation evaluation/internal renderer preparation may still allocate required derived data.

The standard production sync path therefore has no unconditional `clone_for_rendering` before either retained or changed rendering. `LUMAPAINT_PANE_SOURCE_REUSE=0` keeps eager rendering forks/full metadata snapshots as the comparison control. Tiled sources retain their existing owned preparation. Failed standard render or tiled install/render hides the surface and clears the revision reuse stamp; only an advanced presentation serial can install a new stamp. Native visual acceptance is still deferred.

`host.pane_borrowed_renders` counts standard sync requests routed through the borrowed renderer. `host.pane_render_forks_avoided` / `lumapaint-pane-source-avoided` now count **both** retained-picture skips and changed borrowed renders. They must not be treated as GPU-skip counts. `lumapaint-pane-sync-skip` remains the separate retained-presentation signal.

The QA guard still stops on invalid run identity, foreground loss or changed document, now logging a distinct reason and the observed booleans/document IDs. Session-entry failure, missing view and missing window have separate stop labels as well. Multiple guard failures retain their observed fields even though a priority reason is printed. The reason classifier has regression cases for each identity/foreground/document condition and the all-invalid case. Historical unclassified interruptions remain unclassified; this change does not retroactively identify their cause or disable any guard.

🟢 [Done] QA-feature Rust library regression: 224 passed / 2 ignored; warnings-denied all-target QA Clippy, three stage-parser tests, five latency-parser tests, layout model, formatting and core-boundary checks pass. [Follow-up source/check record](pane-borrowed-render-2026-10-25.json).

⭕️ [Pending] The user's request to perform real-app comparison later remains in force. No native input, application launch or new Release timing was run for this follow-up. Its Release rebuild, native pixel/animation/failure-path comparison, multi-window GPU acceptance and physical input measurements remain for that later validation. Prior Release numbers/binary hashes are not attributed to the follow-up.
