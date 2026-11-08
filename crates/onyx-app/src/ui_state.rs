//! What the window remembers between launches: its bounds, whether the
//! sidebar was open, the vault, the note that was open and how it was
//! shown, the vaults opened before. `<home>/state/ui.json`, written when
//! any of it changes (the bounds at most every two seconds), at quit,
//! and when the window's close button closes it: on Windows and Linux
//! closing the last window ends the app without a quit's save. An
//! unreadable file means defaults, never a refusal to start.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use onyx_core::paths;

/// The window's bounds as `window.window_bounds()` gives them, never
/// `bounds()`: on Windows `bounds()` starts under the title bar while a
/// window is opened at a place with its frame above it, so a window
/// saved by `bounds()` opened half a title bar lower at each launch.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiState {
    pub window: Option<Rect>,
    pub sidebar_open: Option<bool>,
    /// How wide the sidebar was dragged.
    pub sidebar_w: Option<f32>,
    /// The settings' section last shown, where they open again.
    pub settings_section: Option<String>,
    /// The vault open, the notes open in it as tabs, and the one showing.
    pub vault: Option<PathBuf>,
    pub tabs: Vec<PathBuf>,
    pub note: Option<PathBuf>,
    /// How notes are shown: `live`, `source` or `reading`. The app's own,
    /// not a note's; only the person changes it.
    pub mode: Option<String>,
    /// The vaults opened before, the latest first, for the landing page.
    pub recent_vaults: Vec<PathBuf>,
    /// The folders folded shut in the sidebar's tree, by their path.
    pub folded: Vec<PathBuf>,
    /// How wide notes run: `narrow`, `middle` or `wide`, the app's own as
    /// `mode` is. Not `wide`, which earlier builds kept as a list of
    /// notes: a file with that list has to go on reading.
    pub page_width: Option<String>,
    /// The folder the last PDF went to, where the next save panel opens.
    /// One since moved or gone is passed over for the note's own.
    pub export_folder: Option<PathBuf>,
    /// Which card the sidebar has open: `notes` or `assets`.
    #[serde(default)]
    pub side_view: Option<String>,
}

fn file() -> std::path::PathBuf {
    paths::state_dir().join("ui.json")
}

/// How many vaults the landing page offers again.
const RECENT: usize = 6;

/// The writer: one thread, the saves in the order asked. `write_atomic`
/// flushes the file to the disk, some 5 ms on an SSD, and a frame is not
/// to wait on that for a tab moved or a folder folded. What was last
/// asked is kept here too and is what `load` gives, so a state read,
/// changed and written again never misses one still on its way.
struct Writer {
    tx: Sender<(UiState, Option<Sender<()>>)>,
    last: Mutex<Option<UiState>>,
}

fn writer() -> &'static Writer {
    static WRITER: OnceLock<Writer> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (tx, rx) = channel::<(UiState, Option<Sender<()>>)>();
        std::thread::Builder::new()
            .name("ui-state".into())
            .spawn(move || {
                for (state, done) in rx {
                    state.write();
                    if let Some(done) = done {
                        let _ = done.send(());
                    }
                }
            })
            .expect("the writer's thread");
        Writer { tx, last: Mutex::new(None) }
    })
}

impl UiState {
    /// What was last asked to be written, else the file.
    pub fn load() -> Self {
        if let Some(last) = writer().last.lock().ok().and_then(|l| l.clone()) {
            return last;
        }
        paths::read_json(&file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    fn write(&self) {
        if let Ok(v) = serde_json::to_value(self) {
            let _ = paths::write_json(&file(), &v);
        }
    }

    fn hand(&self, done: Option<Sender<()>>) {
        let w = writer();
        if let Ok(mut last) = w.last.lock() {
            *last = Some(self.clone());
        }
        let _ = w.tx.send((self.clone(), done));
    }

    /// Written in its turn, with nothing waited on.
    pub fn save_later(&self) {
        self.hand(None);
    }

    /// Written before this returns, after every save asked before it:
    /// for the window's close and the app's end, which must not get
    /// ahead of the last state.
    pub fn save(&self) {
        let (tx, rx) = channel();
        self.hand(Some(tx));
        let _ = rx.recv();
    }

    /// Every save asked so far is on the disk when this returns.
    pub fn flush() {
        if let Some(last) = writer().last.lock().ok().and_then(|l| l.clone()) {
            last.save();
        }
    }

    /// A vault opened: first among the recent ones.
    pub fn remember_vault(&mut self, vault: &std::path::Path) {
        self.recent_vaults.retain(|v| v != vault);
        self.recent_vaults.insert(0, vault.to_path_buf());
        self.recent_vaults.truncate(RECENT);
        self.vault = Some(vault.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::UiState;

    #[test]
    fn a_first_launch_chooses_nothing() {
        assert_eq!(UiState::default().page_width, None);
        assert_eq!(UiState::default().mode, None);
    }

    #[test]
    fn a_file_from_when_wide_was_each_note_s_still_reads() {
        let old = r#"{"tabs":["/v/a.md"],"note":"/v/a.md","mode":"reading","wide":["/v/a.md"]}"#;
        let ui: UiState = serde_json::from_str(old).expect("the file reads");
        assert_eq!(ui.tabs.len(), 1);
        assert_eq!(ui.mode.as_deref(), Some("reading"));
        assert_eq!(ui.page_width, None);
    }
}
