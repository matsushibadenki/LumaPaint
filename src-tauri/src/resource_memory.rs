//! Application-wide memory preferences and scratch-backed undo data.
use lumapaint_core::history_storage::{HistoryBlob, HistoryStore, StoredHistory};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager};
const MIB: u64 = 1024 * 1024;
const RESERVE: u64 = 1024 * MIB;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Disk {
    pub path: String,
    pub enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub ram_percent: f64,
    pub history_states: usize,
    pub disks: Vec<Disk>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            ram_percent: 70.,
            history_states: 50,
            disks: Vec::new(),
        }
    }
}
struct SessionDisk {
    root: PathBuf,
    directory: tempfile::TempDir,
}
pub struct Memory {
    preferences: Mutex<Preferences>,
    sessions: Mutex<Vec<Arc<SessionDisk>>>,
    default_directory: PathBuf,
    total_ram: u64,
    pub scratch_bytes: Arc<AtomicU64>,
    pub resident_history: AtomicU64,
    sequence: AtomicU64,
}
pub struct Resources(pub Arc<Memory>);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    path: String,
    name: String,
    free_bytes: u64,
    startup: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    process_rss: Option<u64>,
    preferences: Preferences,
    total_ram: u64,
    allocated_ram: u64,
    scratch_bytes: u64,
    resident_history: u64,
    drives: Vec<Drive>,
}
impl Resources {
    pub fn load(app: &tauri::AppHandle) -> Result<Self, String> {
        let default_directory = app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("scratch");
        let mut preferences = app
            .path()
            .app_config_dir()
            .ok()
            .and_then(|p| std::fs::read(p.join("performance.json")).ok())
            .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
            .filter(|p| validate(p).is_ok())
            .unwrap_or_default();
        if preferences.disks.is_empty() {
            preferences.disks.push(Disk {
                path: system_root(),
                enabled: true,
            });
        }
        Ok(Self(Arc::new(Memory {
            preferences: Mutex::new(preferences),
            sessions: Mutex::new(Vec::new()),
            default_directory,
            total_ram: physical_memory(),
            scratch_bytes: Arc::new(AtomicU64::new(0)),
            resident_history: AtomicU64::new(0),
            sequence: AtomicU64::new(1),
        })))
    }
}
fn validate(p: &Preferences) -> Result<(), String> {
    if !p.ram_percent.is_finite()
        || !(10.0..=90.0).contains(&p.ram_percent)
        || !(1..=1000).contains(&p.history_states)
        || p.disks.len() > 16
        || p.disks.is_empty()
        || !p.disks.iter().any(|d| d.enabled)
    {
        return Err("Invalid memory or scratch settings / メモリまたは仮想記憶ディスクの設定が無効です / 内存或暂存盘设置无效".into());
    }
    let mut paths = std::collections::HashSet::new();
    for disk in &p.disks {
        if !Path::new(&disk.path).is_absolute() || !paths.insert(&disk.path) {
            return Err("Invalid or duplicate scratch path".into());
        }
    }
    Ok(())
}
impl Memory {
    pub fn cleanup(&self) {
        for session in self.sessions.lock().unwrap().drain(..) {
            let _ = std::fs::remove_dir_all(session.directory.path());
        }
        self.scratch_bytes.store(0, Ordering::Relaxed);
    }
    pub fn policy(&self) -> (usize, usize) {
        let p = self.preferences.lock().unwrap();
        // The history pool is a quarter of the RAM target, shared across editor documents.
        (
            (if self.total_ram == 0 {
                256 * MIB
            } else {
                (self.total_ram as f64 * p.ram_percent / 100.0 / 4.0) as u64
            }) as usize,
            p.history_states,
        )
    }
    pub fn snapshot(&self) -> Snapshot {
        let preferences = self.preferences.lock().unwrap().clone();
        let mut paths = vec![system_root()];
        #[cfg(target_os = "macos")]
        if let Ok(entries) = std::fs::read_dir("/Volumes") {
            for entry in entries.flatten() {
                if entry.path().is_dir()
                    && !entry
                        .path()
                        .canonicalize()
                        .is_ok_and(|path| path == Path::new("/"))
                {
                    paths.push(entry.path().to_string_lossy().into_owned());
                }
            }
        }
        #[cfg(target_os = "windows")]
        for letter in b'A'..=b'Z' {
            let path = format!("{}:\\", letter as char);
            if Path::new(&path).exists() {
                paths.push(path);
            }
        }
        for disk in &preferences.disks {
            if !paths.contains(&disk.path) {
                paths.push(disk.path.clone());
            }
        }
        let drives = paths
            .into_iter()
            .map(|path| {
                let startup = path == system_root();
                Drive {
                    name: if startup {
                        "System volume".into()
                    } else {
                        Path::new(&path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    },
                    free_bytes: free_space(if startup {
                        &self.default_directory
                    } else {
                        Path::new(&path)
                    }),
                    path,
                    startup,
                }
            })
            .collect();
        Snapshot {
            process_rss: lumapaint_renderer::process_rss_bytes(),
            allocated_ram: (self.total_ram as f64 * preferences.ram_percent / 100.0) as u64,
            total_ram: self.total_ram,
            preferences,
            scratch_bytes: self.scratch_bytes.load(Ordering::Relaxed),
            resident_history: self.resident_history.load(Ordering::Relaxed),
            drives,
        }
    }
}
struct Blob {
    _session: Arc<SessionDisk>,
    file: PathBuf,
    bytes: u64,
    checksum: u64,
    usage: Arc<AtomicU64>,
}
impl Drop for Blob {
    fn drop(&mut self) {
        if std::fs::remove_file(&self.file).is_ok() {
            self.usage.fetch_sub(self.bytes, Ordering::Relaxed);
        }
    }
}
impl HistoryBlob for Blob {
    fn read(&self) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let file=std::fs::File::open(&self.file).map_err(|e|format!("Scratch read failed / 仮想記憶ディスクの読み込みに失敗しました / 暂存盘读取失败: {e}"))?;
        if file.metadata().map_err(|e| e.to_string())?.len() != self.bytes {
            return Err("Scratch history size changed".into());
        }
        let mut bytes = Vec::new();
        file.take(self.bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 != self.bytes || checksum(&bytes) != self.checksum {
            return Err("Scratch history is damaged / 退避した履歴データが破損しています / 暂存历史数据已损坏".into());
        }
        Ok(bytes)
    }
}
fn checksum(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}
impl HistoryStore for Memory {
    fn write(&self, bytes: &[u8]) -> Result<StoredHistory, String> {
        let disks = self.preferences.lock().unwrap().disks.clone();
        let mut failures = Vec::new();
        for disk in disks.into_iter().filter(|d| d.enabled) {
            let root = if disk.path == system_root() {
                self.default_directory.clone()
            } else {
                PathBuf::from(&disk.path).join(".lumapaint-scratch")
            };
            let result = (|| -> Result<StoredHistory, String> {
                std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                if free_space(&root) < RESERVE + bytes.len() as u64 {
                    return Err("Scratch disk is full / 仮想記憶ディスクの空き容量が不足しています / 暂存盘空间不足".into());
                }
                let mut sessions = self.sessions.lock().unwrap();
                if !sessions.iter().any(|s| s.root == root) {
                    sessions.push(Arc::new(SessionDisk {
                        root: root.clone(),
                        directory: tempfile::Builder::new()
                            .prefix("session-")
                            .tempdir_in(&root)
                            .map_err(|e| e.to_string())?,
                    }));
                }
                let session = sessions.iter().find(|s| s.root == root).unwrap();
                let path = session.directory.path().join(format!(
                    "history-{}",
                    self.sequence.fetch_add(1, Ordering::Relaxed)
                ));
                let mut file = tempfile::NamedTempFile::new_in(session.directory.path())
                    .map_err(|e| e.to_string())?;
                use std::io::Write;
                file.write_all(bytes)
                    .and_then(|()| file.as_file().sync_all())
                    .map_err(|e| e.to_string())?;
                file.persist_noclobber(&path).map_err(|e| e.to_string())?;
                self.scratch_bytes
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                Ok(StoredHistory(Arc::new(Blob {
                    _session: session.clone(),
                    file: path,
                    bytes: bytes.len() as u64,
                    checksum: checksum(bytes),
                    usage: self.scratch_bytes.clone(),
                })))
            })();
            match result {
                Ok(blob) => return Ok(blob),
                Err(error) => failures.push(format!("{}: {error}", disk.path)),
            }
        }
        Err(failures.join("\n"))
    }
}
fn system_root() -> String {
    #[cfg(target_os = "windows")]
    {
        format!(
            "{}\\",
            std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        "/".into()
    }
}
fn physical_memory() -> u64 {
    #[cfg(target_os = "macos")]
    unsafe {
        let mut bytes = 0u64;
        let mut len = std::mem::size_of::<u64>();
        if libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            return bytes;
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(info) = std::fs::read_to_string("/proc/meminfo") {
        if let Some(bytes) = info.lines().find_map(|line| {
            line.strip_prefix("MemTotal:")
                .and_then(|s| s.split_whitespace().next()?.parse::<u64>().ok())
        }) {
            return bytes * 1024;
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(bytes) = windows_memory() {
            return bytes;
        }
    }
    0
}
fn free_space(path: &Path) -> u64 {
    let mut path = path;
    while !path.exists() {
        let Some(parent) = path.parent() else {
            return 0;
        };
        path = parent;
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return 0;
        };
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) } == 0 {
            let stat = unsafe { stat.assume_init() };
            return stat.f_bavail.saturating_mul(u64::from(stat.f_bsize));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return 0;
        };
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: statvfs writes the advertised initialized structure; path is NUL terminated.
        if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } == 0 {
            let stat = unsafe { stat.assume_init() };
            return (stat.f_bavail as u64).saturating_mul(stat.f_frsize);
        }
    }
    #[cfg(target_os = "windows")]
    {
        return windows_free(path);
    }
    #[allow(unreachable_code)]
    0
}
#[cfg(target_os = "windows")]
fn windows_memory() -> Option<u64> {
    #[repr(C)]
    struct Status {
        length: u32,
        load: u32,
        total: u64,
        available: u64,
        page_total: u64,
        page_available: u64,
        virtual_total: u64,
        virtual_available: u64,
        extended: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(status: *mut Status) -> i32;
    }
    let mut s = Status {
        length: std::mem::size_of::<Status>() as u32,
        load: 0,
        total: 0,
        available: 0,
        page_total: 0,
        page_available: 0,
        virtual_total: 0,
        virtual_available: 0,
        extended: 0,
    };
    (unsafe { GlobalMemoryStatusEx(&mut s) } != 0).then_some(s.total)
}
#[cfg(target_os = "windows")]
fn windows_free(path: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            path: *const u16,
            available: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    if unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } != 0
    {
        available
    } else {
        0
    }
}
#[tauri::command]
pub async fn memory_settings(app: tauri::AppHandle) -> Result<Snapshot, String> {
    let memory = app.state::<Resources>().0.clone();
    crate::diagnostic_jobs::spawn_blocking(move || memory.snapshot())
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn save_memory_settings(
    window: tauri::WebviewWindow,
    preferences: Preferences,
) -> Result<Snapshot, String> {
    validate(&preferences)?;
    let app = window.app_handle().clone();
    let memory = app.state::<Resources>().0.clone();
    let directory = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let result=crate::diagnostic_jobs::spawn_blocking(move||->Result<Snapshot,String>{
        for disk in preferences.disks.iter().filter(|d|d.enabled){
            let path=if disk.path==system_root(){memory.default_directory.clone()}else{PathBuf::from(&disk.path).join(".lumapaint-scratch")};
            std::fs::create_dir_all(&path).map_err(|e|e.to_string())?;
            if free_space(&path)<RESERVE{return Err("Scratch disk needs at least 1 GiB free / 仮想記憶ディスクに1GiB以上の空き容量が必要です / 暂存盘至少需要1GiB可用空间".into());}
            tempfile::NamedTempFile::new_in(&path).map_err(|e|e.to_string())?;
        }
        std::fs::create_dir_all(&directory).map_err(|e|e.to_string())?;
        let mut file=tempfile::NamedTempFile::new_in(&directory).map_err(|e|e.to_string())?;
        use std::io::Write;file.write_all(&serde_json::to_vec(&preferences).map_err(|e|e.to_string())?).and_then(|()|file.as_file().sync_all()).map_err(|e|e.to_string())?;
        file.persist(directory.join("performance.json")).map_err(|e|e.to_string())?;
        *memory.preferences.lock().unwrap()=preferences;
        Ok(memory.snapshot())
    }).await.map_err(|e|e.to_string())??;
    #[cfg(target_os = "macos")]
    crate::canvas::on_main(window, crate::canvas::platform::apply_memory_preferences).await?;
    app.emit("memory-settings-changed", ())
        .map_err(|e| e.to_string())?;
    Ok(result)
}
#[tauri::command]
pub async fn memory_pick_disk() -> Result<Option<String>, String> {
    Ok(rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|file| file.path().to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn memory(directory: &Path, disks: Vec<Disk>) -> Arc<Memory> {
        Arc::new(Memory {
            preferences: Mutex::new(Preferences {
                ram_percent: 70.,
                history_states: 50,
                disks,
            }),
            sessions: Mutex::new(Vec::new()),
            default_directory: directory.join("scratch"),
            total_ram: 1024 * MIB,
            scratch_bytes: Arc::new(AtomicU64::new(0)),
            resident_history: AtomicU64::new(0),
            sequence: AtomicU64::new(1),
        })
    }
    #[test]
    fn scratch_roundtrip_and_clone_lifetime_remove_only_unreferenced_files() {
        let directory = tempfile::tempdir().unwrap();
        let store = memory(
            directory.path(),
            vec![Disk {
                path: system_root(),
                enabled: true,
            }],
        );
        let blob = store.write(b"undo data").unwrap();
        let copy = blob.clone();
        let session = store.sessions.lock().unwrap()[0]
            .directory
            .path()
            .to_owned();
        assert_eq!(store.scratch_bytes.load(Ordering::Relaxed), 9);
        drop(blob);
        drop(store);
        assert_eq!(copy.0.read().unwrap(), b"undo data");
        assert!(session.exists());
        drop(copy);
        assert!(!session.exists());
    }
    #[test]
    fn priority_falls_back_from_unwritable_disk_and_detects_corruption() {
        let directory = tempfile::tempdir().unwrap();
        let blocked = directory.path().join("file");
        std::fs::write(&blocked, b"x").unwrap();
        let store = memory(
            directory.path(),
            vec![
                Disk {
                    path: blocked.to_string_lossy().into_owned(),
                    enabled: true,
                },
                Disk {
                    path: system_root(),
                    enabled: true,
                },
            ],
        );
        let blob = store.write(b"history").unwrap();
        assert_eq!(blob.0.read().unwrap(), b"history");
        let session = store.sessions.lock().unwrap()[0]
            .directory
            .path()
            .to_owned();
        let path = std::fs::read_dir(session)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(path, b"CORRUPT").unwrap();
        assert!(blob.0.read().is_err());
    }
    #[test]
    fn preferences_require_ram_limits_history_limits_and_an_enabled_disk() {
        let mut p = Preferences {
            ram_percent: 70.,
            history_states: 50,
            disks: vec![Disk {
                path: system_root(),
                enabled: true,
            }],
        };
        assert!(validate(&p).is_ok());
        p.ram_percent = f64::NAN;
        assert!(validate(&p).is_err());
        p.ram_percent = 91.;
        assert!(validate(&p).is_err());
        p.ram_percent = 70.;
        p.history_states = 0;
        assert!(validate(&p).is_err());
        p.history_states = 1001;
        assert!(validate(&p).is_err());
        p.history_states = 50;
        p.disks[0].enabled = false;
        assert!(validate(&p).is_err());
        p.disks.clear();
        assert!(validate(&p).is_err());
        assert!(physical_memory() > 0);
        assert!(free_space(&std::env::temp_dir()) > 0);
    }
    #[test]
    fn legacy_history_pages_to_real_disk_and_remains_undoable() {
        let directory = tempfile::tempdir().unwrap();
        let store = memory(
            directory.path(),
            vec![Disk {
                path: system_root(),
                enabled: true,
            }],
        );
        let mut doc = lumapaint_core::document::Document::default();
        for i in 0..6 {
            doc.import_svg_as_layer(format!("Layer {i}"),"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\"/></svg>".into()).unwrap();
        }
        doc.manage_history(&*store, 0, 50).unwrap();
        assert!(store.scratch_bytes.load(Ordering::Relaxed) > 0);
        for count in (0..6).rev() {
            doc.restore_history(false).unwrap();
            doc.undo();
            assert_eq!(doc.svg_layers().count(), count);
        }
        for count in 1..=6 {
            doc.restore_history(true).unwrap();
            doc.redo();
            assert_eq!(doc.svg_layers().count(), count);
        }
        drop(doc);
        assert_eq!(store.scratch_bytes.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn tiled_history_preserves_pixels_and_disk_errors_leave_undo_intact() {
        let directory = tempfile::tempdir().unwrap();
        let store = memory(
            directory.path(),
            vec![Disk {
                path: system_root(),
                enabled: true,
            }],
        );
        let mut doc = lumapaint_core::tiles::TiledRasterDocument::new(2, 2).unwrap();
        doc.add_layer("base".into(), "Base".into()).unwrap();
        for i in 1..=6 {
            doc.write_rect("base", [0, 0, 1, 1], &[i, 0, 0, 255])
                .unwrap();
        }
        doc.manage_history(&*store, 0, 50).unwrap();
        assert!(store.scratch_bytes.load(Ordering::Relaxed) > 0);
        for i in (1..=6).rev() {
            assert_eq!(doc.layers()[0].tiles.pixel(0, 0).unwrap()[0], i);
            doc.undo().unwrap();
        }
        assert_eq!(doc.layers()[0].tiles.pixel(0, 0), Some([0, 0, 0, 0]));
        for i in 1..=6 {
            doc.redo().unwrap();
            assert_eq!(doc.layers()[0].tiles.pixel(0, 0).unwrap()[0], i);
        }
        doc.manage_history(&*store, 0, 50).unwrap();
        doc.undo().unwrap();
        doc.undo().unwrap();
        let before = doc.state();
        let revision = doc.revision();
        for session in store.sessions.lock().unwrap().iter() {
            for file in std::fs::read_dir(session.directory.path()).unwrap() {
                std::fs::remove_file(file.unwrap().path()).unwrap();
            }
        }
        assert!(doc.undo().is_err());
        assert_eq!(doc.state(), before);
        assert_eq!(doc.revision(), revision);
        assert!(doc.can_undo());
    }
}
