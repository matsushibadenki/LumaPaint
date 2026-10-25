# Timed continuous drawing / pan — Release comparison, 2026-10-25

The final implementation keeps the established 32ms pending-input / 50ms background-service eligibility instead of the rejected held-gesture 8ms/16ms candidate. A pending request retains its oldest time until the active renderer's presentation serial changes. With no fresh input for 80ms, normal background budget returns even while a button is held. Unchanged inactive standard panes reuse successful GPU presentations; geometry, visibility and masks still update, while changed document revision/viewport, hidden/recreated/failed surfaces and tiled documents bypass reuse. Independent timeline ticks continue rendering animation.

## Scope and protocol

Apple M4/Metal, isolated `app.lumapaint.pane-animation-qa`, 1200×800, nine 640×480 owned fixtures with 100 vector objects each. Documents 2–9 independently play a 24fps loop; document 1 is static at 100% and receives brush input on a temporary pixel layer, followed by hand-tool pan. The timeline panel is collapsed. This is a small reproducible workload, not complex artwork.

Each of four separate app launches uses the same feature-enabled optimized binary, with `LUMAPAINT_PANE_INPUT_PRIORITY=1`. `LUMAPAINT_PANE_CONTINUOUS_PRIORITY=0/1/1/0` compares the old policy/no-reuse control with oldest-request policy/retained-presentation reuse. Pan event instrumentation and zero-delta suppression are common to both modes. No compilation runs during measurement.

Each drawing/pan phase has sixteen one-second gestures, sixty movement events per gesture, 150ms intergesture gaps. The first two gestures per phase are warmup. Each mode therefore contains two runs and 28 retained gestures per operation. Many adjacent presentations come from the same gesture and are correlated; sample counts are not independent trial counts. Drawing and the temporary layer are undone before pan; alternating pan returns to its stored starting view. Five seconds of idle playback follow pan. QA changes are discarded without saving fixture files.

The user authorized the dedicated timed driver. `native-input-qa` is absent from normal builds. A producer thread schedules timestamped AppKit NSEvents through Tauri's main-thread dispatcher and NSApplication's queue; it never calls document mutation methods directly. Application identity, document, foreground, tool and small native canvas bounds are checked. Switching away cancels generation-specific queued producer work. Command-Shift-F10 reads a strict bounded plan at `/tmp/lumapaint-native-gesture-plan.json`, for example:

```json
{"tag":"final-A1-draw","kind":"draw","gestures":16,"seconds":1}
```

The native factory rounds some timestamp conversions by 1ns, so matching uses ±10ns. All sixteen down/up pairs, exactly sixty moves and 1000ms source-time spans must be present; incomplete captures fail the summarizer rather than silently entering the result. A visible one-second pilot verified native drawing before comparison. Short CUA drag sequences (0.16–0.84ms) and an external Quartz probe (zero delivered events) are excluded; see [transport evidence](continuous-input-validation-2026-10-25.md).

`first_handler` includes the oldest input coalesced into a presentation; `latest_handler` uses its most recent input. Both end at the renderer's presentation API call and exclude physical display latency. The matched NSEvent age plus latest-handler time is an **upper estimate** because probes overlap; it is not an exact queue-plus-render sum. Held-background progression counts changed frame IDs between received down/up records per nominal source-held second, deduplicating forced presentations of the same frame. Idle frequency uses capture wall time. CPU time is for the Rust application only, with one core = 100%.

## Rejected candidate

The earlier 8ms/16ms policy completed four valid trials but increased pooled pan first-handler p95 from 1.626ms to 17.670ms. One drawing trial had a 975.803ms first-handler maximum. It is not the final policy. The evidence is retained in [the rejected-candidate JSON](continuous-input-rejected-policy-2026-10-25.json). Concurrent macOS/other-project load was observed and left untouched; idle frequency also varied, so the candidate's nominal drawing p95 reduction is not presented as a general improvement.

## Reproduction

Build with the isolated identifier and opt-in QA feature:

```sh
TAURI_CONFIG='{"identifier":"app.lumapaint.pane-animation-qa","productName":"LumaPaint Pane Animation QA"}' CARGO_BUILD_JOBS=1 cargo build -p lumapaint --release --features tauri/custom-protocol,native-input-qa --locked --offline
```

Create the nine fixtures with `crates/lumapaint-formats/examples/pane_animation_fixture.rs`, use the isolated QA app/bundle and prepare the layout/playback through the UI. Do not enable this driver in a production app or point it at user artwork. Capture phase byte ranges and start/end wall times into records with `tag`, `mode`, `kind`, `planTag`, `start`, `end`, `trialLog`; record process CPU/RSS separately. Summarize with:

```sh
python3 scripts/summarize-continuous-input.py RECORDS.json PROCESS.jsonl RESULT.json --binary QA_BINARY
```

The JSON companion records binary/source hashes, phase log hashes, source event counts, per-gesture/presentation distributions, background advancement, CPU/RSS and stored views. Raw phase evidence remains in `/tmp/lumapaint-continuous-*`; completed full runtime logs are gzip-compressed to avoid retaining hundreds of megabytes of idle metrics.

## Final comparison — completed after manual unlock

🟢 [Done] Four accepted launches completed in off/on/on/off order: A1, B1, B2, A2retry. All 128 gestures delivered down, exactly sixty moves and up over 1000ms source time (7,680 moves). Sixteen warmup gestures were discarded, leaving 28 gestures per operation per mode. The first attempt at A2 lost foreground after the second gesture's down event; the guard cancelled it. That incomplete attempt is documented in JSON and excluded for missing source events, not because of its timing. A2retry restarted the app and complete workload from the same unchanged fixture files.

The same final binary SHA-256 is `a6e49026f1561b36bf331ffcbeeae2a37fcffc968623a474eb1bd4646704ae2a`. Target and QA bundle hashes match. Rust source hashes are in [the final JSON](continuous-input-release-2026-10-25.json), separate from the rejected candidate.

### Pooled retained presentations

All values below are milliseconds; p95 uses nearest rank. These are two runs per mode, not thousands of independent experiments.

| Operation / metric | Control off | Final on |
| --- | ---: | ---: |
| Drawing first-handler p95 | 2.800 | 3.315 |
| Drawing latest-handler p95 | 2.784 | 3.220 |
| Drawing matched event + handler upper-estimate p95 | 21.853 | 17.848 |
| Drawing retained presentations | 1544 | 1673 |
| Pan first-handler p95 | 0.832 | 0.872 |
| Pan latest-handler p95 | 0.801 | 0.850 |
| Pan matched event + handler upper-estimate p95 | 12.354 | 12.351 |
| Pan retained presentations | 1644 | 1666 |

Drawing handler p95 increased slightly (latest 2.784→3.220ms; first 2.800→3.315ms), while the matched event-plus-handler upper-estimate p95 decreased (21.853→17.848ms). Its median increased (14.059→15.679ms), and phase-sensitive queue wait prevents treating that p95 as a universal improvement. Pan first-handler p95 is 0.832→0.872ms and the matched upper-estimate is effectively unchanged (12.354→12.351ms). The rejected candidate's ~17.7ms first-handler p95 does not recur with the conservative policy. **The result verifies avoided redundant rendering and absence of that large pan regression; it does not demonstrate a general input-latency or CPU improvement.**

### Per-run retained timing and load

CPU covers the Rust process over the entire phase, including warmup, with one core = 100%. Held progression is per nominal retained source-held second.

| Run / operation | Latest p95 ms | First p95 ms | Upper-estimate p95 ms | CPU % | Background held progression /s |
| --- | ---: | ---: | ---: | ---: | ---: |
| final-A1-draw | 3.023 | 3.079 | 22.195 | 44.3 | 7.64–8.14 |
| final-A1-pan | 0.809 | 0.855 | 7.372 | 41.0 | 7.50–8.00 |
| final-B1-draw | 3.213 | 3.352 | 12.729 | 42.7 | 7.57–8.00 |
| final-B1-pan | 0.696 | 0.696 | 12.958 | 39.1 | 7.50–8.00 |
| final-B2-draw | 3.258 | 3.258 | 18.356 | 43.5 | 7.43–7.79 |
| final-B2-pan | 0.897 | 0.950 | 6.669 | 40.5 | 7.57–7.79 |
| final-A2retry-draw | 2.359 | 2.359 | 16.122 | 39.3 | 7.64–8.43 |
| final-A2retry-pan | 0.781 | 0.801 | 12.789 | 38.9 | 7.43–8.07 |

🟢 [Done] Each enabled drawing phase skipped 128 redundant inactive-pane renders (all sixteen stroke completions × eight unchanged panes; this counter includes warmup). Control phases skipped zero. The native screenshot shows actual brush marks on the temporary layer; undo restored two layers before pan. All eight background documents advanced during each held phase. Enabled idle captures recovered to about 23.34–23.75 changed frames/s per pane; all-mode idle range was 22.57–23.75. This is below the requested 24fps under measured overhead and is not a hard rate guarantee.

🟢 [Done] Stored document-1 zoom and pan matched exactly before/after all four accepted pan phases. All nine fixture hashes remain unchanged. All four accepted apps and the interrupted attempt exited with code 0 after discarding QA changes. Short-run Rust RSS peaks were roughly 216–254MiB; this does not validate long-term retention or include WebView/GPU memory.

🟢 [Done] Final conservative source: Rust library 222 passed / 2 ignored; QA-feature all-target Clippy with warnings denied and optimized QA Release succeeded. Formatting, architecture boundaries, layout tests and five latency-parser tests passed. The continuous summarizer accepts complete captures and rejects deliberately incomplete source sequences.

🟠 [Next] Profile pre-handler queue wait and display-callback phase, move source-fork preparation behind a safe unchanged-pane check, and compare longer drawing/pan on complex artwork. Physical mouse/stylus, keyboard/WebView edits, 60-second × five-trial runs and long-term memory remain unverified. Windows/Linux and multiple-display/DPI acceptance remain Pending. The manual-lock blocker for this four-trial comparison is resolved.

Cleanup: all accepted and interrupted QA apps exited normally; the duplicate temporary QA bundle was removed. Completed final runtime logs were gzip-compressed, reclaiming about 176MiB. Per-phase logs, process samples and source/build hashes remain available; no fixture or user artwork was changed.
