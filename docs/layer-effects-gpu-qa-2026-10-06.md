# Layer effects GPU batch verification — 2026-10-06

[Done] Renderer effect batches use wgpu for tone, saturation/vibrance, smooth/linear curves, HSL mixing and grading. Core supplies prepared f32 tone tables, curve tangents and grading tints; document storage is unchanged. Pixel payloads stay in Rust and do not cross JavaScript IPC.

The backend accepts premultiplied RGBA8 batches of at least 262,144 pixels within device and memory limits. Cached input/output/readback buffers use power-of-two capacities, with retained GPU buffers capped at 64 MiB including configuration. This is not an application-wide CPU/GPU memory budget. Larger batches, unsupported devices and failed dispatches use the existing CPU implementation. Initialization runs in a background thread; CPU processing continues until the GPU is ready. Failed requests leave caller pixels untouched. Bindings and readback cover the actual request length when cached buffers are reused for smaller images.

The viewport/cache and thumbnail renderer paths use this backend where eligible. Core tile composition and PSD/PSB baking remain on the prepared CPU path.

## Quality checks

Hardware tests cover five configurations, including extreme tonal values, 16-point curves, mixed smooth/linear curves, HSL mixing and grading. Batches of 1, 17, 8,192, 65,536 and 262,144 pixels exercise buffer growth and shrinkage. Alpha values 0, 1, 64, 128 and 255 are preserved exactly, including untouched transparent RGB bytes. Each tested RGB component differs from the CPU reference by at most one 8-bit level; cross-backend byte identity is not claimed. Public asynchronous initialization and CPU continuation until readiness also pass.

## Release measurements

Warm backend, five-run medians on the local macOS GPU. GPU times include configuration preparation, upload, dispatch, readback and copying results into the destination. Device/pipeline initialization, rasterization, UI latency and IPC are excluded.

| Effects | Pixels | CPU ms | GPU ms | Speedup |
| --- | ---: | ---: | ---: | ---: |
| Tone and saturation | 262,144 | 6.621 | 1.561 | 4.24× |
| Curves, mixer and grading | 262,144 | 18.471 | 4.115 | 4.49× |
| Tone and saturation | 1,048,576 | 21.165 | 4.522 | 4.68× |
| Curves, mixer and grading | 1,048,576 | 72.731 | 10.847 | 6.71× |

Reproduce with `cargo test -p lumapaint-renderer --release --lib benchmark_effects_gpu_and_cpu_with_transfers --locked -- --ignored --nocapture`. Hardware quality test: `cargo test -p lumapaint-renderer --lib gpu_effects_match_cpu_alpha_and_rounding_after_growth_and_smaller_batches --locked -- --ignored --nocapture`.

## Regression verification

All 663 workspace tests pass; 36 hardware/benchmark tests remain ignored in the normal run. The GPU hardware quality test and Release benchmark above were run separately and passed. Workspace Clippy with warnings denied, formatting, dependency boundary checks and diff checks pass. The macOS debug application bundle builds successfully. The running application was not restarted; interactive end-to-end latency remains unmeasured. No frontend controls were changed.

## Resident input follow-up

[Done] Workspace raw rasters receive unique process-local revision IDs. Effect/opacity updates and Undo/Redo retain the ID; geometry changes, viewport changes, cache clearing and rebuilt rasters receive a new ID. The GPU keeps the most recent source ID and length alongside its existing input buffer. Matching requests upload configuration only, without hashing or comparing the complete image. Unidentified batches invalidate the identity. GPU errors also invalidate it. IDs are runtime cache metadata and are not serialized.

Only the most recent input is resident: interleaved work for another image requires another upload. The existing 64 MiB GPU budget is unchanged. Output readback, CPU opacity application and final canvas upload still occur; this is not yet a GPU-only display path.

The separate hardware test `resident_source_skips_uploads_and_invalidates_after_other_work` verifies five settings changes on one 262,144-pixel source with only the initial upload, including returning to an earlier setting. CPU comparisons retain the one-level RGB tolerance and exact alpha. Unidentified work, different revisions, shorter inputs and returning to the original input each cause the expected upload. Workspace regression checks preserve revisions through effects/Undo/Redo/opacity and renew them after clearing. All 663 workspace tests also pass after this follow-up; the new resident-input hardware test and updated Release benchmark pass separately.

Follow-up Release run, five-run medians (warm pipeline; configuration, dispatch, readback and destination copy included):

| Effects | Pixels | Re-upload input ms | Resident input ms | Input bytes across five resident updates |
| --- | ---: | ---: | ---: | ---: |
| Tone and saturation | 262,144 | 1.547 | 1.346 | 0 |
| Curves, mixer and grading | 262,144 | 4.056 | 3.899 | 0 |
| Tone and saturation | 1,048,576 | 4.527 | 2.829 | 0 |
| Curves, mixer and grading | 1,048,576 | 5.809 | 5.331 | 0 |

Timings vary with GPU load and clocks; the transfer-byte assertion is the deterministic acceptance check. These are batch timings, not interactive frame-rate measurements.

[Next] GPU output passed directly to display without readback; resident inputs for multiple layers; GPU tile composition and export baking; histogram reuse and end-to-end input/display latency measurements.

[Later] A shared application-wide CPU/GPU cache budget and long-session peak-memory measurements.
