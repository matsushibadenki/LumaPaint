# Input priority versus background playback — 2026-10-25

The change gives a native input frame priority over inactive-pane playback. Production scheduling does not depend on diagnostic input markers: a nested interaction scope marks actual redraw requests from native pointer editing, pan and zoom. Each editor window records the active renderer's presentation serial and input time. Until that serial advances, the background timer yields for up to 32ms; a 50ms eligibility limit prevents repeated new inputs from indefinitely withholding all background service. Recent input uses a cooperative 2ms background batch target for 80ms, then restores the existing 8ms target. The existing oldest-serviced ordering rotates panes. A single expensive pane is not preempted, so these targets are not hard execution limits or per-document frame-rate guarantees. Closing a window drops its priority/service records.

Native Release acceptance uses Apple M4/Metal, real AppKit/WKWebView at 1200×800, scale 1, nine 640×480 documents with 100 vector rectangles each. Document 1 is static at 100%, and the other eight play a 24fps/48-frame X-motion loop. The existing [fixture generator](../crates/lumapaint-formats/examples/pane_animation_fixture.rs) supplies all nine files. Their file hashes remain unchanged. No browser/IPC mocks or synthetic Rust input are used.

All four trials use the **same Release binary** and separate processes. Only the diagnostic process override differs: `LUMAPAINT_PANE_INPUT_PRIORITY=0` disables the policy for the control; `1` enables it. Production defaults to enabled. Both modes retain the same tracing and event-age probe. This compares the scheduling policy, not an uninstrumented old release against a new release. Order is control A1, enabled B1, enabled B2, control A2. Builds and checks finish before timing. Each process reopens the same saved files, starts the same eight transports, warms their native surfaces, and closes the timeline panel before measurement.

Each trial performs 16 native zoom-in/out cycles at window point (190,220). The tool is checked before each click, and every 100%→125%→100% transition is checked through accessibility. Each trial has exactly 32 native presentations; four warmup presentations are discarded, leaving 28 retained samples per trial and 56 per mode. Trials take about 37–51 seconds including automation/AX observations. These are short, sparse-input acceptance observations, not the outstanding controlled 60-seconds×5-trials or continuous-pen gate.

| Trial | Priority | Handler→present median / p95 | Event-age probe p95 | Paired event→present upper-estimate p95 | Main CPU | Background present calls/sec per document |
| --- | --- | --- | --- | --- | --- | --- |
| A1 | disabled | 14.67 / 20.01ms | 8.80ms | 28.28ms | 70.8% | 19.1–19.4 |
| B1 | enabled | 9.37 / 17.02ms | 10.79ms | 20.24ms | 55.0% | 22.7–22.8 |
| B2 | enabled | 8.02 / 12.10ms | 1.58ms | 12.89ms | 54.2% | 22.5–22.8 |
| A2 | disabled | 12.56 / 23.52ms | 5.93ms | 29.05ms | 56.7% | 23.4–23.5 |

| Pooled retained samples | Disabled (56) | Enabled (56) |
| --- | --- | --- |
| Handler→present median | 13.931ms | 8.251ms |
| Handler→present p95 | 21.383ms | 17.015ms |
| Handler→present maximum | 23.667ms | 17.216ms |
| Event-age probe p95 | 8.755ms | 10.752ms |
| Paired event→present upper-estimate p95 | 29.049ms | 20.235ms |

Handler time ends at the present API call; it excludes OS queueing, GPU completion and physical display. The new event-age probe subtracts `NSEvent.timestamp` from system uptime at an early pointer-handler probe. It includes dispatch and any work before that probe, so it is not exclusively OS queue time. Only ages matching the latest retained presentation input ID are included (56/56 per mode). The paired estimate adds that event's age to its handler interval. It contains the overlap between the host marker and probe, so it is an upper estimate rather than calibrated input-to-photon latency. **Separately computed p95 values are never added together.**

The handler p95 is about 20% lower in this bounded comparison; both enabled trials also have lower paired-estimate p95 than either control trial. This supports keeping the policy for this fixture, but does not establish a general speedup. In particular, the event-age probe tail alone does not improve, and trial-to-trial phase/automation variation is substantial. A1 has higher CPU and lower background cadence than A2 despite identical control settings; no causal CPU/FPS improvement is claimed. Enabled playback remains live on all eight panes, with a small cadence tradeoff versus A2. Continuous gestures can defer background work more strongly and need separate acceptance.

Native traces confirm the policy actually runs: B1 has 5 yield / 153 limited-budget ticks; B2 has 32 yield / 128 limited-budget ticks; controls have none. A pending input frame is not counted as completed merely because the background surface presents: the serial belongs to the active renderer. No document pixels or history cross IPC, and the policy does not mutate canonical documents. CPU/RSS samples cover only the Rust main process; 100% CPU means one core and RSS is not total VRAM, WebView or WindowServer memory.

[Machine-readable evidence](input-latency-pane-priority-2026-10-25.json) preserves binary/source hashes, per-trial tool/zoom proofs, matched event IDs and ages, raw presentation samples, background frame sequences, policy counts and process samples. Runtime logs use `/tmp/lumapaint-input-priority-{A1,B1,B2,A2}-runtime.log.gz`; per-trial captures use `/tmp/lumapaint-input-priority-*-trial.log`. The isolated QA app exits normally in all four runs and its temporary bundle is removed. Combined runtime logs are compressed after verification to reduce SSD use.

Validation: Rust library 220 passed / 2 ignored, including deadline/fairness and nested interaction-scope tests. The latency summarizer has five passing tests, including latest-event correlation, warmup exclusion and unmatched ages. Host all-target Clippy with warnings denied, Release build, formatting, architecture boundaries, document-layout model and diff checks pass. Native acceptance covers mouse zoom; continuous painting, pan/pinch latency, keyboard/WebView property edits, text/effects/pixel-cel works, long sessions, Windows/Linux and multi-display/DPI changes remain to be measured.
