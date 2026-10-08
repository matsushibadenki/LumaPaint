//! Publish each synchronous blocking task before a pooled worker is reused.
//! The batch guard is created on the worker, never held across an async await.
pub(crate) fn spawn_blocking<F, R>(work: F) -> tauri::async_runtime::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let _metrics = lumapaint_core::performance::batch("host.blocking_job");
        work()
    })
}
