# Document pane playback / Release input observations — 2026-10-25

Native acceptance uses an isolated Tauri Release bundle (`app.lumapaint.pane-animation-qa`), embedded UI, real WKWebView/AppKit input, Apple M4/Metal, 1200×800, scale 1. No IPC mocks or synthetic Rust input injection are used. All builds/tests finished before accepted timing. Initial app-automation attachment took about twelve minutes and is excluded.

The generated fixture has nine saved 640×480 documents with 100 overlapping vector rectangles each. Each vector layer has X keys at frames 0/24/47 with values 0/90/0, 24fps, 48 frames, looping. Document 1 remains static and is the sole input target. Documents 2–9 are played, then parked by switching tabs. All eight inactive surfaces are created and warmed before timing. Inactive documents use their own fit view; document 1 starts each accepted trial at 100%. The source is [pane_animation_fixture.rs](../crates/lumapaint-formats/examples/pane_animation_fixture.rs).

Each condition has two accepted trials, 16 actual native zoom-in/out cycles (32 presentations) per trial. Every tool transition and every 100%→125%→100% result is checked through accessibility. The first four presentations per trial are discarded, leaving 56 samples per condition. An early automation attempt that clicked before the native tool direction changed is excluded entirely. Order: 1 pane A/B, 4 panes A/B, 9 panes A/B. Each trial lasts about 30 seconds. These are bounded acceptance observations, not the separate 60-seconds×5-trials performance gate.

| Visible panes / animated inactive panes | Handler→present median, two trials | p95, two trials | Maximum | Rust process CPU | Inactive observed fps |
| --- | --- | --- | --- | --- | --- |
| 1 / 0 | 10.22 / 10.62 ms | 12.57 / 14.85 ms | 17.18 ms | 3.4–3.7% | no inactive presentations |
| 4 / 3 | 10.66 / 11.13 ms | 17.00 / 15.54 ms | 17.47 ms | 26.5–26.6% | about 23.5 per document |
| 9 / 8 | 12.22 / 11.64 ms | 17.01 / 17.05 ms | 26.73 ms | 53.4–54.8% | 23.3–23.5 per document |

Timings measure native handler start to the present API call. They exclude OS queue delay, GPU completion and physical display. CPU is cumulative Rust-process CPU time divided by wall time (100% = one core), sampled with `ps`; it excludes WebView and WindowServer. Main-process RSS stays around 254MiB across these already-warmed trials. Hidden surfaces remain retained, so this does not measure fresh allocation cost or total VRAM. The active viewport is smaller as pane count rises, so this is a real-layout comparison, not a fixed-viewport causal benchmark.

Inactive per-frame evaluate/render host p95 is 3.78ms in both four-pane trials and 3.02/2.84ms in the nine-pane trials. The common scheduler checks an 8ms cooperative batch budget between panes and rotates the starting pane. One expensive pane can exceed that target (observed single-pane host maxima 16.08ms in four panes and 10.31ms in nine panes); it is not a hard deadline. Wall-clock playback skips obsolete frames, never accumulates a catch-up queue, and does not repaint held pictures. Eight visible animations remain close to 24fps in this fixture. Hidden tab animations keep their clock but produce no presentations in the one-pane trials.

GPU timestamp availability is incomplete and varies by surface, including only three valid samples in each one-pane trial. The JSON retains valid GPU samples but they are not used to infer the active canvas's GPU latency or physical frame rate. This run establishes functionality and increased host load with more animated panes; it does not establish an old/new speedup or statistical causality. Dense real artwork, pixel-cel animation, complex text/effects, stylus input, Windows/Linux and multi-display transitions need further acceptance.

[Machine-readable evidence](input-latency-document-panes-2026-10-25.json) contains the measured binary hash, raw native presentation records, individual zoom-state proofs, per-document frame sequences and process samples. Logs use `/tmp/lumapaint-pane-trial-*.log`, `/tmp/lumapaint-pane-animation-runtime.log.gz` and `/tmp/lumapaint-pane-process-samples.jsonl`. Screenshots use `/tmp/lumapaint-pane-animation-nine-{a,b,final}.png`.

A follow-up correction moves the active document clock into the same native scheduler so switching a playing pane into the editor also keeps it moving when the timeline panel is collapsed. The six timing trials above precede that active-clock correction; their binary hash remains explicitly recorded. Final-build native verification is recorded below separately.


## Final Release verification

The final binary adds the native active-document clock and preserves animation ownership on a surface-acquisition retry. Its SHA-256 is stored separately as `final_binary_sha256`; every JSON trial carries its own binary hash. Two additional accepted 16-cycle trials (`final-one`, `final-nine`) use this final binary after all compilation has finished, again discarding four of 32 presentations per trial.

| Final binary condition | Retained samples | Median | p95 | Maximum | Main-process CPU | Median RSS |
| --- | --- | --- | --- | --- | --- | --- |
| 1 pane / 0 inactive animations visible | 28 | 10.995ms | 17.404ms | 17.792ms | 3.5% | 227MiB |
| 9 panes / 8 inactive animations visible | 28 | 7.556ms | 17.121ms | 17.156ms | 58.3% | 228MiB |

All eight final-build inactive panes show about 23.2–23.5 present calls per second. The final p95 values are similar; neither the median ordering nor the earlier p95 difference supports a reliable monotonic latency regression claim. CPU load clearly rises in this fixture, while valid host timing still excludes OS queue delay. RSS differences between processes/build runs are not a memory improvement claim. These final observations remain short acceptance trials rather than the outstanding 60-seconds×5-trials gate.

A separate native check switches a playing pane into the active editor while the timeline panel remains collapsed. The active renderer continues presenting without frontend timeline polling. In the first common-clock acceptance run, the active presentation count advanced from 170 to 626 and the colored object's bounds moved from (195,629)–(249,673) to (193,629)–(247,673). The final binary repeats this check; screenshots use `/tmp/lumapaint-pane-verified-active-{a,b}.png` and the final nine-pane image `/tmp/lumapaint-pane-animation-verified-nine.png`. Closing/reopening the panel retains its established stop behavior for the active transport; other parked transports are independent.

Rust library regression: 218 passed, 2 ignored. The 17 timeline tests include document-clock/preview/pause retention and canonical revision preservation. Final Clippy (all targets, warnings denied), UI production build, layout model, latency summarizer tests, architecture boundaries, formatting and diff checks pass. Native fixtures remain unmodified by paint operations. The QA app exits normally and its temporary bundle is removed after acceptance.

The three combined QA runtime logs are gzip-archived after verification to reduce temporary SSD use. Per-trial logs and JSON raw presentation samples remain available.
