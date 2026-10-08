//! Optional bounded, asynchronous GPU frame timestamps. Never wait for readback.
use std::sync::{
    atomic::{AtomicU64, AtomicU8, Ordering},
    Arc,
};
static NEXT_STREAM: AtomicU64 = AtomicU64::new(1);
const SLOTS: usize = 3;
const WINDOW: usize = 512;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Sample {
    pub frame_id: u64,
    pub gpu_ns: u64,
    pub host_ns: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Distribution {
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub over_60hz: usize,
    pub over_120hz: usize,
}
#[derive(Debug)]
pub(crate) struct Summary {
    pub samples: usize,
    pub attempts: u64,
    pub completed: u64,
    pub busy: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub gpu: Distribution,
    pub host: Distribution,
}
struct History {
    samples: [Sample; WINDOW],
    count: usize,
    next: usize,
    completed: u64,
}
impl Default for History {
    fn default() -> Self {
        Self {
            samples: [Sample::default(); WINDOW],
            count: 0,
            next: 0,
            completed: 0,
        }
    }
}
impl History {
    fn push(&mut self, sample: Sample) {
        self.samples[self.next] = sample;
        self.next = (self.next + 1) % WINDOW;
        self.count = (self.count + 1).min(WINDOW);
        self.completed += 1;
    }
    fn distribution(&self, value: impl Fn(&Sample) -> u64) -> Distribution {
        let mut values = [0; WINDOW];
        for (dst, sample) in values.iter_mut().zip(&self.samples[..self.count]) {
            *dst = value(sample);
        }
        let values = &mut values[..self.count];
        values.sort_unstable();
        // Nearest rank, with integer arithmetic and a defined empty window.
        let percentile = |percent: usize| {
            if values.is_empty() {
                0
            } else {
                values[(values.len() * percent).div_ceil(100) - 1]
            }
        };
        Distribution {
            p50_ns: percentile(50),
            p95_ns: percentile(95),
            p99_ns: percentile(99),
            over_60hz: values.iter().filter(|&&ns| ns > 16_666_666).count(),
            over_120hz: values.iter().filter(|&&ns| ns > 8_333_333).count(),
        }
    }
}
struct Slot {
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    frame_id: u64,
    host_ns: u64,
}
pub(crate) struct FrameTiming {
    pub stream_id: u64,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: [Slot; SLOTS],
    period: f64,
    next_frame: u64,
    busy: u64,
    failed: u64,
    cancelled: u64,
    history: History,
    last_reported: u64,
}
impl FrameTiming {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        Some(Self {
            stream_id: NEXT_STREAM.fetch_add(1, Ordering::Relaxed),
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("Frame timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: (SLOTS * 2) as u32,
            }),
            resolve: crate::gpu_metrics::create_buffer!(
                device,
                &wgpu::BufferDescriptor {
                    label: Some("Frame timestamp resolve"),
                    size: SLOTS as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }
            ),
            slots: std::array::from_fn(|_| Slot {
                readback: crate::gpu_metrics::create_buffer!(
                    device,
                    &wgpu::BufferDescriptor {
                        label: Some("Frame timestamp readback"),
                        size: 16,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    }
                ),
                state: Arc::new(AtomicU8::new(0)),
                frame_id: 0,
                host_ns: 0,
            }),
            period: f64::from(queue.get_timestamp_period()),
            next_frame: 1,
            busy: 0,
            failed: 0,
            cancelled: 0,
            history: History::default(),
            last_reported: 0,
        })
    }
    pub fn collect(&mut self, device: &wgpu::Device) -> [Option<Sample>; SLOTS] {
        let _ = device.poll(wgpu::PollType::Poll);
        let mut samples = [None; SLOTS];
        for (index, slot) in self.slots.iter().enumerate() {
            match slot.state.load(Ordering::Acquire) {
                2 => {
                    let view = crate::gpu_metrics::mapped_read!(slot.readback.slice(..))
                        .expect("successful timestamp mapping");
                    let start = u64::from_le_bytes(view[..8].try_into().unwrap());
                    let end = u64::from_le_bytes(view[8..16].try_into().unwrap());
                    if end >= start {
                        let sample = Sample {
                            frame_id: slot.frame_id,
                            gpu_ns: ((end - start) as f64 * self.period) as u64,
                            host_ns: slot.host_ns,
                        };
                        self.history.push(sample);
                        samples[index] = Some(sample);
                    } else {
                        self.failed += 1;
                        eprintln!(
                            "lumapaint-gpu-frame stream={} id={} status=invalid_timestamp",
                            self.stream_id, slot.frame_id
                        );
                        crate::performance::count("gpu_timestamp_invalid_samples", 1);
                    }
                    drop(view);
                    slot.readback.unmap();
                    slot.state.store(0, Ordering::Release);
                }
                3 => {
                    self.failed += 1;
                    eprintln!(
                        "lumapaint-gpu-frame stream={} id={} status=readback_error",
                        self.stream_id, slot.frame_id
                    );
                    crate::performance::count("gpu_timestamp_readback_errors", 1);
                    slot.state.store(0, Ordering::Release);
                }
                _ => {}
            }
        }
        samples
    }
    pub fn reserve(&mut self) -> Option<usize> {
        let frame_id = self.next_frame;
        self.next_frame += 1;
        let index = self.slots.iter().position(|slot| {
            slot.state
                .compare_exchange(0, 4, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        });
        if let Some(index) = index {
            self.slots[index].frame_id = frame_id;
            self.slots[index].host_ns = 0;
        } else {
            self.busy += 1;
            eprintln!(
                "lumapaint-gpu-frame stream={} id={} status=busy",
                self.stream_id, frame_id
            );
        }
        index
    }
    pub fn record_host(&mut self, slot: usize, nanoseconds: u64) {
        self.slots[slot].host_ns = nanoseconds;
    }
    pub fn correlation(&self, slot: usize) -> (u64, u64) {
        (self.stream_id, self.slots[slot].frame_id)
    }
    pub fn summary(&self) -> Summary {
        Summary {
            samples: self.history.count,
            attempts: self.next_frame - 1,
            completed: self.history.completed,
            busy: self.busy,
            failed: self.failed,
            cancelled: self.cancelled,
            gpu: self.history.distribution(|s| s.gpu_ns),
            host: self.history.distribution(|s| s.host_ns),
        }
    }
    pub fn take_summary(&mut self) -> Option<Summary> {
        if self.history.completed == 0
            || (self.last_reported != 0 && self.history.completed - self.last_reported < 60)
        {
            return None;
        }
        self.last_reported = self.history.completed;
        Some(self.summary())
    }
    pub fn writes(&self, slot: usize, start: bool) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: start.then_some(slot as u32 * 2),
            end_of_pass_write_index: (!start).then_some(slot as u32 * 2 + 1),
        }
    }
    fn compute_writes(&self, slot: usize) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(slot as u32 * 2),
            end_of_pass_write_index: Some(slot as u32 * 2 + 1),
        }
    }
    pub fn resolve(&self, slot: usize, encoder: &mut wgpu::CommandEncoder) {
        let offset = slot as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
        encoder.resolve_query_set(
            &self.queries,
            slot as u32 * 2..slot as u32 * 2 + 2,
            &self.resolve,
            offset,
        );
        crate::gpu_metrics::copy_buffer_to_buffer!(
            encoder,
            &self.resolve,
            offset,
            &self.slots[slot].readback,
            0,
            16
        );
    }
    pub fn submitted(&self, slot: usize) {
        let state = Arc::clone(&self.slots[slot].state);
        state.store(1, Ordering::Release);
        self.slots[slot]
            .readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                state.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
            });
    }
}

/// Use optional timestamp queries without altering ordinary device requirements.
pub(crate) async fn request_job_device(
    adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    let required_features = if crate::performance::enabled()
        && adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY)
    {
        wgpu::Features::TIMESTAMP_QUERY
    } else {
        wgpu::Features::empty()
    };
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features,
            ..Default::default()
        })
        .await
}

pub(crate) struct JobTiming {
    kind: &'static str,
    timing: std::cell::RefCell<Option<Box<FrameTiming>>>,
}

impl JobTiming {
    pub fn new(kind: &'static str, device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            kind,
            timing: std::cell::RefCell::new(
                crate::performance::enabled()
                    .then(|| FrameTiming::new(device, queue).map(Box::new))
                    .flatten(),
            ),
        }
    }
    pub fn collect(&self, device: &wgpu::Device) {
        let Ok(mut timing) = self.timing.try_borrow_mut() else {
            return;
        };
        if let Some(timing) = &mut *timing {
            for sample in timing.collect(device).into_iter().flatten() {
                eprintln!(
                    "lumapaint-gpu-job kind={} stream={} id={} gpu_ns={} encoding_host_ns={}",
                    self.kind, timing.stream_id, sample.frame_id, sample.gpu_ns, sample.host_ns
                );
            }
            if let Some(summary) = timing.take_summary() {
                eprintln!(
                    "lumapaint-gpu-job-summary kind={} stream={} cancelled={} {:?}",
                    self.kind, timing.stream_id, summary.cancelled, summary
                );
            }
        }
    }
    pub fn start(&self, device: &wgpu::Device) -> Option<JobSample<'_>> {
        self.collect(device);
        let mut timing = self.timing.try_borrow_mut().ok()?;
        let slot = timing.as_mut()?.reserve()?;
        Some(JobSample {
            timing,
            slot,
            start: std::time::Instant::now(),
        })
    }
}

pub(crate) struct JobSample<'a> {
    timing: std::cell::RefMut<'a, Option<Box<FrameTiming>>>,
    slot: usize,
    start: std::time::Instant,
}

impl JobSample<'_> {
    pub fn writes(&self) -> wgpu::ComputePassTimestampWrites<'_> {
        self.timing.as_ref().unwrap().compute_writes(self.slot)
    }
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let timing = self.timing.as_mut().unwrap();
        timing.resolve(self.slot, encoder);
        timing.record_host(
            self.slot,
            self.start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
        );
    }
    pub fn submitted(self) {
        self.timing.as_ref().unwrap().submitted(self.slot);
    }
}

impl Drop for JobSample<'_> {
    fn drop(&mut self) {
        // An early return before submit must not leak a reserved measurement slot.
        // Pending mappings are never cancelled or reused prematurely.
        let timing = self.timing.as_mut().unwrap();
        if timing.slots[self.slot]
            .state
            .compare_exchange(4, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            timing.cancelled += 1;
            crate::performance::count("gpu_timestamp_cancelled_jobs", 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires timestamp-capable GPU and LUMAPAINT_RENDER_METRICS=1"]
    fn gpu_compute_jobs_cancel_early_returns_and_collect_correlated_samples() {
        assert!(crate::performance::enabled());
        let (device, queue) = pollster::block_on(async {
            let adapter = wgpu::Instance::default()
                .request_adapter(&Default::default())
                .await
                .unwrap();
            assert!(adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY));
            request_job_device(&adapter).await.unwrap()
        });
        let timing = JobTiming::new("test_compute", &device, &queue);
        for _ in 0..8 {
            drop(timing.start(&device).unwrap());
        }
        {
            let timer = timing.timing.borrow();
            let summary = timer.as_ref().unwrap().summary();
            assert_eq!(summary.cancelled, 8);
            assert_eq!(summary.busy, 0);
        }
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mut job = timing.start(&device).unwrap();
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                timestamp_writes: Some(job.writes()),
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.dispatch_workgroups(1, 1, 1);
        }
        job.resolve(&mut encoder);
        queue.submit([encoder.finish()]);
        job.submitted();
        // Test only. Production collection never adds a GPU completion wait.
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        timing.collect(&device);
        let timer = timing.timing.borrow();
        let summary = timer.as_ref().unwrap().summary();
        assert_eq!(summary.attempts, 9);
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.cancelled, 8);
        assert_eq!(summary.failed, 0);
        assert!(summary.host.p50_ns > 0);
    }
    #[test]
    fn percentiles_use_nearest_rank_and_deadline_counts() {
        let mut history = History::default();
        assert_eq!(history.distribution(|s| s.gpu_ns).p99_ns, 0);
        for ns in [1, 8_333_333, 8_333_334, 16_666_666, 16_666_667] {
            history.push(Sample {
                frame_id: ns,
                gpu_ns: ns,
                host_ns: ns * 2,
            });
        }
        let distribution = history.distribution(|s| s.gpu_ns);
        assert_eq!(
            distribution,
            Distribution {
                p50_ns: 8_333_334,
                p95_ns: 16_666_667,
                p99_ns: 16_666_667,
                over_60hz: 1,
                over_120hz: 3
            }
        );
        assert_eq!(history.distribution(|s| s.host_ns).over_60hz, 3);
    }
    #[test]
    fn rolling_window_discards_old_samples_and_has_fixed_capacity() {
        let mut history = History::default();
        history.push(Sample {
            frame_id: 1,
            gpu_ns: u64::MAX,
            host_ns: u64::MAX,
        });
        for id in 2..WINDOW as u64 + 2 {
            history.push(Sample {
                frame_id: id,
                gpu_ns: 10,
                host_ns: 20,
            });
        }
        assert_eq!(history.count, WINDOW);
        assert_eq!(history.completed, WINDOW as u64 + 1);
        let distribution = history.distribution(|s| s.gpu_ns);
        assert_eq!(distribution.p99_ns, 10);
        assert_eq!(distribution.over_60hz, 0);
        assert_eq!(history.distribution(|s| s.host_ns).p95_ns, 20);
    }
    #[test]
    #[ignore = "requires a GPU with timestamp queries"]
    fn gpu_timestamps_are_bounded_and_readback_is_reused() {
        let (device, queue) = pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance.request_adapter(&Default::default()).await.unwrap();
            assert!(adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY));
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::TIMESTAMP_QUERY,
                    ..Default::default()
                })
                .await
                .unwrap()
        });
        let mut timing = FrameTiming::new(&device, &queue).unwrap();
        assert_ne!(timing.stream_id, 0);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        for batch in 0..2 {
            let slots: Vec<_> = (0..SLOTS).map(|_| timing.reserve().unwrap()).collect();
            assert!(
                timing.reserve().is_none(),
                "busy ring must skip, never wait or grow"
            );
            for slot in slots.into_iter().rev() {
                let mut encoder = device.create_command_encoder(&Default::default());
                for start in [true, false] {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        timestamp_writes: Some(timing.writes(slot, start)),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: if start {
                                    wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                                } else {
                                    wgpu::LoadOp::Load
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                }
                timing.resolve(slot, &mut encoder);
                queue.submit([encoder.finish()]);
                timing.submitted(slot);
                timing.record_host(slot, (batch * 4 + slot + 1) as u64 * 1000);
            }
            // Waiting is confined to the test, never production collection.
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let samples = timing.collect(&device);
            for (index, sample) in samples.into_iter().enumerate() {
                let sample = sample.unwrap();
                assert!(sample.gpu_ns > 0);
                assert_eq!(sample.frame_id, (batch * 4 + index + 1) as u64);
                assert_eq!(sample.host_ns, sample.frame_id * 1000);
            }
            let summary = timing.summary();
            assert_eq!(summary.samples, (batch + 1) * SLOTS);
            assert_eq!(summary.attempts, ((batch + 1) * 4) as u64);
            assert_eq!(summary.busy, (batch + 1) as u64);
            assert_eq!(summary.failed, 0);
            assert_eq!(summary.host.p99_ns, (batch * 4 + SLOTS) as u64 * 1000);
            if batch == 0 {
                assert!(timing.take_summary().is_some());
            }
            assert!(
                timing.take_summary().is_none(),
                "do not report unchanged or undersized batches repeatedly"
            );
            assert!(timing
                .collect(&device)
                .into_iter()
                .all(|sample| sample.is_none()));
        }
    }
}
