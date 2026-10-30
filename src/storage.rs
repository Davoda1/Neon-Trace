//! Per-user configuration and explicitly enabled bounded local history.
use crate::export::{ExportBundle, now_ms};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_HISTORY_RECORDS: usize = 100;
pub const MAX_HISTORY_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_RECORD_BYTES: u64 = 8 * 1024 * 1024;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub language: String,
    pub window_width: f32,
    pub window_height: f32,
    pub sidebar_visible: bool,
    pub history_enabled: bool,
    pub tcp_check: bool,
    pub reduced_effects: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            language: "en".into(),
            window_width: 1280.0,
            window_height: 800.0,
            sidebar_visible: true,
            history_enabled: false,
            tcp_check: false,
            reduced_effects: false,
        }
    }
}
impl Config {
    pub fn normalize(&mut self) {
        if !["en", "de", "hu", "fr", "it", "ru"].contains(&self.language.as_str()) {
            self.language = "en".into();
        }
        self.window_width = if self.window_width.is_finite() {
            self.window_width.clamp(680.0, 7680.0)
        } else {
            1280.0
        };
        self.window_height = if self.window_height.is_finite() {
            self.window_height.clamp(540.0, 4320.0)
        } else {
            800.0
        };
    }
}
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub history_dir: PathBuf,
}
// The Windows KnownFolder APIs used by dirs do not honor APPDATA overrides.
// Prefer explicit absolute environment paths for portable/test profiles.
#[cfg(any(windows, test))]
fn prefer_directory_override(
    value: Option<std::ffi::OsString>,
    fallback: Option<PathBuf>,
) -> Option<PathBuf> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or(fallback)
}
fn per_user_bases() -> (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) {
    #[cfg(windows)]
    {
        (
            prefer_directory_override(std::env::var_os("APPDATA"), dirs::config_dir()),
            prefer_directory_override(std::env::var_os("LOCALAPPDATA"), dirs::cache_dir()),
            prefer_directory_override(std::env::var_os("LOCALAPPDATA"), dirs::data_local_dir()),
        )
    }
    #[cfg(not(windows))]
    {
        (
            dirs::config_dir(),
            dirs::cache_dir(),
            dirs::data_local_dir(),
        )
    }
}
impl Paths {
    pub fn discover() -> io::Result<Self> {
        let (config_base, cache_base, data_base) = per_user_bases();
        let config_base = config_base.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Per-user configuration directory is unavailable",
            )
        })?;
        let cache_base = cache_base.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Per-user cache directory is unavailable",
            )
        })?;
        let data_base = data_base.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Per-user data directory is unavailable",
            )
        })?;
        Ok(Self {
            config_dir: config_base.join("NeonTrace"),
            cache_dir: cache_base.join("NeonTrace").join("geodata"),
            history_dir: data_base.join("NeonTrace").join("history"),
        })
    }
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }
    pub fn load_config(&self) -> io::Result<Config> {
        let path = self.config_file();
        if !path.exists() {
            return Ok(Config::default());
        }
        let mut file = File::open(path)?;
        if file.metadata()?.len() > 64 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Settings file exceeds its size limit",
            ));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut config: Config = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        config.normalize();
        Ok(config)
    }
    pub fn save_config(&self, config: &Config) -> io::Result<()> {
        let mut safe = config.clone();
        safe.normalize();
        atomic_write(&self.config_file(), |file| {
            serde_json::to_writer_pretty(&mut *file, &safe).map_err(io::Error::other)?;
            file.write_all(b"\n")
        })
    }
}

/// Write to a new neighbouring file and flush it before replacing the destination.
/// A partial serialization or disk-full error leaves the old destination intact.
pub fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "A file name is required"))?
        .to_string_lossy();
    let temp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temp, path)?;
        #[cfg(unix)]
        {
            if let Ok(directory) = File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}
#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: String,
    pub saved_unix_ms: u64,
    pub demonstration: bool,
    pub targets: Vec<String>,
    pub size_bytes: u64,
}
#[derive(Debug, Clone)]
pub struct HistoryStore {
    directory: PathBuf,
    pub enabled: bool,
}
impl HistoryStore {
    pub fn new(directory: PathBuf, enabled: bool) -> Self {
        Self { directory, enabled }
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    /// No directory, session, or target is written while history is disabled.
    pub fn record(&self, bundle: &ExportBundle) -> io::Result<Option<String>> {
        if !self.enabled {
            return Ok(None);
        }
        let bytes = serde_json::to_vec_pretty(bundle).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "History snapshot exceeds its size limit",
            ));
        }
        let id = format!(
            "{}-{}-{}",
            now_ms(),
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        atomic_write(&self.path_for(&id)?, |file| file.write_all(&bytes))?;
        self.prune()?;
        Ok(Some(id))
    }
    pub fn list(&self) -> io::Result<Vec<HistoryEntry>> {
        let mut entries = Vec::new();
        for (id, path, size) in self.files()? {
            if size > MAX_RECORD_BYTES {
                continue;
            }
            let bytes = fs::read(path)?;
            if let Ok(bundle) = serde_json::from_slice::<ExportBundle>(&bytes) {
                entries.push(HistoryEntry {
                    id,
                    saved_unix_ms: bundle.exported_unix_ms,
                    demonstration: bundle.demonstration,
                    targets: bundle.sessions.iter().map(|s| s.target.clone()).collect(),
                    size_bytes: size,
                });
            }
        }
        entries.sort_by(|a, b| {
            b.saved_unix_ms
                .cmp(&a.saved_unix_ms)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(entries)
    }
    pub fn load(&self, id: &str) -> io::Result<ExportBundle> {
        let path = self.path_for(id)?;
        if fs::metadata(&path)?.len() > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "History snapshot exceeds its size limit",
            ));
        }
        let bundle: ExportBundle =
            serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)?;
        if bundle.schema_version != crate::export::SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported history schema version",
            ));
        }
        Ok(bundle)
    }
    pub fn delete(&self, id: &str) -> io::Result<()> {
        fs::remove_file(self.path_for(id)?)
    }
    pub fn clear(&self) -> io::Result<()> {
        for (_, path, _) in self.files()? {
            fs::remove_file(path)?;
        }
        Ok(())
    }
    fn path_for(&self, id: &str) -> io::Result<PathBuf> {
        if id.is_empty() || id.len() > 100 || !id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid history identifier",
            ));
        }
        Ok(self.directory.join(format!("{id}.json")))
    }
    fn files(&self) -> io::Result<Vec<(String, PathBuf, u64)>> {
        if !self.directory.exists() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
                continue;
            };
            if self.path_for(&id).is_err() {
                continue;
            }
            files.push((id, path, entry.metadata()?.len()));
        }
        Ok(files)
    }
    fn prune(&self) -> io::Result<()> {
        let mut files = self.files()?;
        // IDs begin with a decimal millisecond timestamp; all currently generated
        // timestamps have the same width. Parse so ordering remains chronological.
        files.sort_by_key(|(id, _, _)| {
            id.split('-')
                .next()
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0)
        });
        let mut bytes: u64 = files.iter().map(|f| f.2).sum();
        let mut count = files.len();
        for (_, path, size) in files {
            if count <= MAX_HISTORY_RECORDS && bytes <= MAX_HISTORY_BYTES {
                break;
            }
            fs::remove_file(path)?;
            count -= 1;
            bytes = bytes.saturating_sub(size);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn temporary(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "neontrace-storage-{label}-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }
    #[test]
    fn explicit_appdata_paths_override_known_folder_fallbacks() {
        let root = temporary("profile");
        let fallback = root.join("known-folder");
        let isolated = root.join("isolated");
        assert_eq!(
            prefer_directory_override(
                Some(isolated.clone().into_os_string()),
                Some(fallback.clone())
            ),
            Some(isolated)
        );
        assert_eq!(
            prefer_directory_override(Some(std::ffi::OsString::new()), Some(fallback.clone())),
            Some(fallback.clone())
        );
        assert_eq!(
            prefer_directory_override(None, Some(fallback.clone())),
            Some(fallback.clone())
        );
        assert_eq!(
            prefer_directory_override(
                Some(std::ffi::OsString::from("relative-profile")),
                Some(fallback.clone())
            ),
            Some(fallback)
        );
    }
    #[test]
    fn defaults_do_not_save_session_information() {
        let root = temporary("privacy");
        let history = HistoryStore::new(root.join("history"), false);
        let bundle = ExportBundle {
            sessions: vec![crate::export::Session {
                target: "private-host.example".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(history.record(&bundle).unwrap().is_none());
        assert!(!root.exists());
        let text = serde_json::to_string(&Config::default()).unwrap();
        assert!(!text.contains("target"));
        assert!(!text.contains("theme"));
        assert!(!text.contains("origin"));
        assert!(!text.contains("speed"));
        assert!(!text.contains("map"));
    }
    #[test]
    fn failed_atomic_write_retains_last_file() {
        let root = temporary("atomic");
        let path = root.join("settings.json");
        atomic_write(&path, |f| f.write_all(b"last-good")).unwrap();
        assert!(
            atomic_write(&path, |f| {
                f.write_all(b"partial")?;
                Err(io::Error::other("interrupted"))
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"last-good");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn history_is_bounded_and_has_explicit_delete_clear() {
        let root = temporary("history");
        let history = HistoryStore::new(root.clone(), true);
        let bundle = ExportBundle::default();
        for _ in 0..MAX_HISTORY_RECORDS + 2 {
            history.record(&bundle).unwrap();
        }
        let entries = history.list().unwrap();
        assert_eq!(entries.len(), MAX_HISTORY_RECORDS);
        let id = &entries[0].id;
        assert_eq!(history.load(id).unwrap().schema_version, 1);
        history.delete(id).unwrap();
        assert_eq!(history.list().unwrap().len(), MAX_HISTORY_RECORDS - 1);
        assert!(history.load("../settings").is_err());
        history.clear().unwrap();
        assert!(history.list().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn config_normalizes_unsupported_values_without_target_state() {
        let mut config = Config {
            language: "xx".into(),
            window_width: 1.0,
            window_height: f32::NAN,
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.language, "en");
        assert_eq!(config.window_width, 680.0);
        assert_eq!(config.window_height, 800.0);
    }
}
