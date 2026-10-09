//! Host-owned backing storage; the document core never opens files or queries the OS.
use std::sync::Arc;
pub trait HistoryBlob: Send + Sync {
    fn read(&self) -> Result<Vec<u8>, String>;
}
pub trait HistoryStore: Send + Sync {
    fn write(&self, bytes: &[u8]) -> Result<StoredHistory, String>;
}
#[derive(Clone)]
pub struct StoredHistory(pub Arc<dyn HistoryBlob>);
impl std::fmt::Debug for StoredHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StoredHistory")
    }
}
impl PartialEq for StoredHistory {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
