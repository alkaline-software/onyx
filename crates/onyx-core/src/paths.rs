//! Where Onyx keeps its files: one folder, `~/.onyx`, which
//! `ONYX_HOME` overrides (the tests and a probe copy use it, so neither
//! touches the person's own). On an iPhone and an iPad it is
//! `Library/Application Support/Onyx` in the app's own container, the one
//! place there an app may make a folder that is backed up and that Files
//! does not show.
//!
//! Every write is a temp file and a rename, so a reader sees the old file
//! or the new one and never half of one.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// iCloud Drive's folder on this machine, when iCloud Drive is set up:
/// a vault made in it is synced by the system, with no account of
/// Onyx's own. On a Mac it is `~/Library/Mobile Documents/com~apple~CloudDocs`,
/// on Windows the folder iCloud for Windows keeps; Linux has none.
pub fn icloud_drive() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        home().join("Library/Mobile Documents/com~apple~CloudDocs")
    } else if cfg!(target_os = "windows") {
        home().join("iCloudDrive")
    } else {
        return None;
    };
    dir.is_dir().then_some(dir)
}

/// The person's documents folder, where a vault goes when no other
/// place is chosen; the home folder when the system names none.
pub fn documents() -> PathBuf {
    dirs::document_dir().unwrap_or_else(home)
}

/// The data folder.
pub fn root() -> PathBuf {
    match std::env::var_os("ONYX_HOME").filter(|v| !v.is_empty()) {
        Some(p) => PathBuf::from(p),
        None if cfg!(target_os = "ios") => home().join("Library/Application Support/Onyx"),
        None => home().join(".onyx"),
    }
}

pub fn config_file() -> PathBuf {
    root().join("config.json")
}

/// What the window and the core remember between launches.
pub fn state_dir() -> PathBuf {
    root().join("state")
}

/// What can be thrown away and fetched again: downloads.
pub fn cache_dir() -> PathBuf {
    root().join("cache")
}

pub fn ensure_dirs() -> std::io::Result<()> {
    for dir in [root(), state_dir(), cache_dir()] {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

pub fn write_json(path: &Path, data: &Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(data)?;
    text.push('\n');
    write_atomic(path, text.as_bytes())
}

/// Write through a sibling temp file and a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!("{name}.tmp{}", std::process::id()));
    let result = (|| {
        let mut fh = fs::File::create(&tmp)?;
        fh.write_all(bytes)?;
        fh.sync_all().ok();
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
