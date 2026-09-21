//! One writer per application profile; coalesced snapshots are encoded and written off-thread.
use crate::project_file;
use lumapaint_core::document::Document;
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub saved_revision: Option<u64>,
    pub pending: bool,
    pub error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub modified_ms: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub status: Status,
    pub candidates: Vec<Candidate>,
}

enum Job {
    Save {
        document: Box<Document>,
        revision: u64,
        source: Option<PathBuf>,
    },
    Clear {
        source: Option<PathBuf>,
    },
}
#[derive(Default)]
struct Shared {
    pending: Option<Job>,
    stop: bool,
    status: Status,
}

pub struct Recovery {
    directory: PathBuf,
    current: PathBuf,
    source: Option<PathBuf>,
    shared: Arc<(Mutex<Shared>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    // Keep the exclusive profile lock until all writes complete.
    _lock: File,
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn candidate_id(id: &str) -> bool {
    id.starts_with("session-")
        && id.ends_with(".lumapaint")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

impl Recovery {
    pub fn start(directory: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("writer.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|e| {
            format!("Recovery directory is already in use or cannot be locked: {e}")
        })?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let current = directory.join(format!("session-{}-{stamp}.lumapaint", std::process::id()));
        let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
        let worker_state = shared.clone();
        let destination = current.clone();
        let worker = std::thread::Builder::new()
            .name("lumapaint-recovery".into())
            .spawn(move || loop {
                let job = {
                    let (mutex, wake) = &*worker_state;
                    let mut state = mutex.lock().unwrap();
                    while state.pending.is_none() && !state.stop {
                        state = wake.wait(state).unwrap();
                    }
                    if state.pending.is_none() && state.stop {
                        break;
                    }
                    state.pending.take().unwrap()
                };
                let (result, revision) = match job {
                    Job::Save {
                        mut document,
                        revision,
                        source,
                    } => {
                        let result = document
                            .encode()
                            .and_then(|bytes| project_file::write(&destination, &bytes))
                            .and_then(|_| source.as_deref().map_or(Ok(()), remove_if_present));
                        (result, Some(revision))
                    }
                    Job::Clear { source } => {
                        let result = remove_if_present(&destination)
                            .and_then(|_| source.as_deref().map_or(Ok(()), remove_if_present));
                        (result, None)
                    }
                };
                let mut state = worker_state.0.lock().unwrap();
                state.status.pending = state.pending.is_some();
                match result {
                    Ok(()) => {
                        state.status.saved_revision = revision;
                        state.status.error = None;
                    }
                    Err(error) => {
                        state.status.error = Some(error);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            directory,
            current,
            source: None,
            shared,
            worker: Some(worker),
            _lock: lock,
        })
    }
    fn enqueue(&self, job: Job) {
        let (mutex, wake) = &*self.shared;
        let mut state = mutex.lock().unwrap();
        if state.stop {
            return;
        }
        state.pending = Some(job);
        state.status.pending = true;
        wake.notify_one();
    }
    pub fn checkpoint(&self, document: Document) {
        let snapshot = document.snapshot();
        if snapshot.dirty {
            self.enqueue(Job::Save {
                document: Box::new(document),
                revision: snapshot.revision,
                source: self.source.clone(),
            });
        } else {
            self.clear();
        }
    }
    pub fn clear(&self) {
        self.enqueue(Job::Clear {
            source: self.source.clone(),
        });
    }
    pub fn info(&self) -> Result<Info, String> {
        let mut candidates = Vec::new();
        for entry in std::fs::read_dir(&self.directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !candidate_id(&id)
                || entry.path() == self.current
                || !entry.file_type().map_err(|e| e.to_string())?.is_file()
            {
                continue;
            }
            let modified_ms = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |t| t.as_millis() as u64);
            candidates.push(Candidate { id, modified_ms });
        }
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.modified_ms));
        Ok(Info {
            status: self.shared.0.lock().unwrap().status.clone(),
            candidates,
        })
    }
    pub fn read_candidate(&self, id: &str) -> Result<Document, String> {
        if !candidate_id(id) {
            return Err("Invalid recovery identifier".into());
        }
        let path = self.directory.join(id);
        if path == self.current
            || std::fs::symlink_metadata(&path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("Invalid recovery file".into());
        }
        project_file::read(&path)
    }
    pub fn delete_candidate(&self, id: &str) -> Result<(), String> {
        if !candidate_id(id) {
            return Err("Invalid recovery identifier".into());
        }
        let path = self.directory.join(id);
        if path == self.current {
            return Err("The active recovery copy cannot be deleted".into());
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err("Invalid recovery file".into());
        }
        remove_if_present(&path)
    }
    pub fn delete_all_candidates(&self) -> Result<(), String> {
        let ids = self
            .info()?
            .candidates
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        for id in ids {
            self.delete_candidate(&id)?;
        }
        Ok(())
    }
    pub fn adopt(&mut self, id: &str) {
        self.source = Some(self.directory.join(id));
    }
    pub fn stop(&mut self) {
        {
            let mut state = self.shared.0.lock().unwrap();
            state.stop = true;
            self.shared.1.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Recovery {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::{Brush, Point};
    fn drawing() -> Document {
        let mut doc = Document::default();
        doc.begin(Point { x: 30.0, y: 40.0 }, Brush::default())
            .unwrap();
        doc.finish();
        doc
    }
    #[test]
    fn restart_discovers_checkpoint_and_restores_unsaved_work() {
        let directory = tempfile::tempdir().unwrap();
        let mut first = Recovery::start(directory.path().into()).unwrap();
        first.checkpoint(drawing());
        first.stop();
        drop(first);
        let next = Recovery::start(directory.path().into()).unwrap();
        let info = next.info().unwrap();
        assert_eq!(info.candidates.len(), 1);
        let candidate = next.read_candidate(&info.candidates[0].id).unwrap();
        let mut doc = Document::default();
        doc.replace_recovered(candidate);
        assert_eq!(doc.snapshot().stroke_count, 1);
        assert!(doc.snapshot().dirty);
        assert!(doc.snapshot().file_name.is_none());
    }
    #[test]
    fn clear_cannot_be_overtaken_by_an_older_pending_write() {
        let directory = tempfile::tempdir().unwrap();
        let mut recovery = Recovery::start(directory.path().into()).unwrap();
        for _ in 0..20 {
            recovery.checkpoint(drawing());
        }
        recovery.clear();
        recovery.stop();
        assert!(!recovery.current.exists());
        assert!(recovery.info().unwrap().status.saved_revision.is_none());
    }
    #[test]
    fn second_writer_is_rejected_and_other_candidates_survive_clear() {
        let directory = tempfile::tempdir().unwrap();
        let older = directory.path().join("session-older.lumapaint");
        project_file::write(&older, &drawing().encode().unwrap()).unwrap();
        let mut recovery = Recovery::start(directory.path().into()).unwrap();
        assert!(Recovery::start(directory.path().into()).is_err());
        recovery.clear();
        recovery.stop();
        assert!(older.exists());
    }
    #[test]
    fn failed_write_keeps_source_and_reports_error() {
        let directory = tempfile::tempdir().unwrap();
        let id = "session-older.lumapaint";
        let source = directory.path().join(id);
        project_file::write(&source, &drawing().encode().unwrap()).unwrap();
        let mut recovery = Recovery::start(directory.path().into()).unwrap();
        std::fs::create_dir(&recovery.current).unwrap();
        recovery.adopt(id);
        recovery.checkpoint(drawing());
        recovery.stop();
        assert!(source.exists());
        assert!(recovery.info().unwrap().status.error.is_some());
    }
    #[test]
    fn recovery_rejects_traversal_and_malformed_candidate() {
        let directory = tempfile::tempdir().unwrap();
        let recovery = Recovery::start(directory.path().into()).unwrap();
        assert!(recovery
            .read_candidate("../session-other.lumapaint")
            .is_err());
        std::fs::write(directory.path().join("session-bad.lumapaint"), b"truncated").unwrap();
        assert!(recovery.read_candidate("session-bad.lumapaint").is_err());
    }
    #[test]
    fn successful_adoption_keeps_one_recoverable_copy() {
        let directory = tempfile::tempdir().unwrap();
        let id = "session-old.lumapaint";
        project_file::write(&directory.path().join(id), &drawing().encode().unwrap()).unwrap();
        let mut recovery = Recovery::start(directory.path().into()).unwrap();
        let mut restored = Document::default();
        restored.replace_recovered(recovery.read_candidate(id).unwrap());
        recovery.adopt(id);
        recovery.checkpoint(restored);
        recovery.stop();
        assert!(!directory.path().join(id).exists());
        assert_eq!(
            project_file::read(&recovery.current)
                .unwrap()
                .snapshot()
                .stroke_count,
            1
        );
    }

    #[test]
    fn candidates_can_be_deleted_individually_or_together() {
        let directory = tempfile::tempdir().unwrap();
        for id in ["session-one.lumapaint", "session-two.lumapaint"] {
            project_file::write(&directory.path().join(id), &drawing().encode().unwrap()).unwrap();
        }
        let recovery = Recovery::start(directory.path().into()).unwrap();
        recovery.delete_candidate("session-one.lumapaint").unwrap();
        assert_eq!(recovery.info().unwrap().candidates.len(), 1);
        recovery.delete_all_candidates().unwrap();
        assert!(recovery.info().unwrap().candidates.is_empty());
        assert!(recovery.delete_candidate("../writer.lock").is_err());
    }
}
