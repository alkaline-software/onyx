//! The settings: defaults, then `~/.onyx/config.json`, then the
//! environment. Loading never fails; a file that does not parse is the
//! defaults. Every field has a default, so a file written by an older
//! version, or edited by hand with a field missing, still loads.

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    /// `system` follows the OS; `light` and `dark` pin one look.
    pub appearance: String,
    /// The accent, by its key in the app's `look::ACCENTS`.
    pub accent: String,
    /// `system`, or a language's code. On a Mac the app's own
    /// `AppleLanguages` is the setting and this is not read.
    pub language: String,
    /// Ask for the newest release once a day and say so in Settings.
    /// Nothing is installed without the person's press.
    pub check_updates: bool,
    /// The person's name, as the vault's page greets them and the
    /// sidebar's foot shows them; empty, the account's first name.
    pub user_name: String,
    /// The person's picture: the path of the copy kept under the app's
    /// own folder, or empty for their initial on the accent.
    pub avatar: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { appearance: "system".into(), accent: "teal".into(), language: "system".into(), check_updates: true, user_name: String::new(), avatar: String::new() }
    }
}

/// How a note is set in the editor and as it reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EditorConfig {
    /// The face a note's words are set in, by its key in the app's
    /// `fonts::SERIFS` or `fonts::SANS`.
    pub text_face: String,
    /// The face code is set in, by its key in `fonts::MONOS`.
    pub mono_face: String,
    /// The face Chinese, Japanese and Korean are set in behind the Latin
    /// faces, by its key in `fonts::CJK`: `auto` matches the note's face
    /// (a serif behind a serif), else a carried face.
    pub cjk_face: String,
    /// A font installed on the machine, by its family name, in place of
    /// the chosen face; empty, the chosen face. One for the words, one
    /// for code, one for Chinese.
    pub text_custom: String,
    pub mono_custom: String,
    pub cjk_custom: String,
    /// `small`, `medium`, `large` or `custom`: a preset for every place
    /// at once, or the person's own sizes (`sizes`).
    pub text_size: String,
    /// The person's own sizes, kept while a preset is chosen so Custom
    /// comes back to them; `None` until Custom was chosen once.
    pub sizes: Option<TextSizes>,
    /// Whether a note's spelling and grammar are marked.
    pub check_writing: bool,
    /// The language they are checked in, by its key in `check::LANGUAGES`.
    pub writing_language: String,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self { text_face: "inter".into(), mono_face: "jetbrains".into(), cjk_face: "auto".into(), text_custom: String::new(), mono_custom: String::new(), cjk_custom: String::new(), text_size: "medium".into(), sizes: None, check_writing: true, writing_language: "en-US".into() }
    }
}

impl EditorConfig {
    /// The sizes in force: the preset's, or the person's own under Custom.
    pub fn text_sizes(&self) -> TextSizes {
        TextSizes::preset(&self.text_size).unwrap_or_else(|| self.sizes.unwrap_or(TextSizes::MEDIUM).clamped())
    }
}

/// The size of the words in each place, in points.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TextSizes {
    /// A note's words, as it is written and as it reads.
    pub note: f32,
    /// Code: a block, a span, a table, a note as written.
    pub code: f32,
}

impl Default for TextSizes {
    fn default() -> Self {
        Self::MEDIUM
    }
}

impl TextSizes {
    /// The default: a note at Obsidian's own body size, code a step
    /// under, as a Mac's editors set monospace. Every preset's sizes are
    /// whole points.
    pub const MEDIUM: Self = Self { note: 16., code: 14. };
    pub const SMALL: Self = Self { note: 14., code: 13. };
    pub const LARGE: Self = Self { note: 18., code: 16. };
    /// What a size may be set to.
    pub const LEAST: f32 = 9.;
    pub const MOST: f32 = 32.;

    /// A preset by its name; `custom` and anything else is none.
    pub fn preset(name: &str) -> Option<Self> {
        match name {
            "small" => Some(Self::SMALL),
            "medium" => Some(Self::MEDIUM),
            "large" => Some(Self::LARGE),
            _ => None,
        }
    }

    /// A step up or down from `value`: a whole point.
    pub fn step(value: f32, up: bool) -> f32 {
        let to = if up { value.floor() + 1. } else { value.ceil() - 1. };
        to.clamp(Self::LEAST, Self::MOST)
    }

    pub fn clamped(self) -> Self {
        Self { note: self.note.clamp(Self::LEAST, Self::MOST), code: self.code.clamp(Self::LEAST, Self::MOST) }
    }
}

/// How a note is set as a PDF, as the export sheet last left it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PdfConfig {
    /// `a3`, `a4`, `a5`, `letter`, `legal` or `tabloid`; empty, what the
    /// person's region prints on (`pdf::paper_of_locale`).
    pub paper: String,
    pub landscape: bool,
    /// `small`, `normal` or `large`.
    pub margins: String,
    /// The face the words and code are set in, by their keys in the app's
    /// `fonts`; empty, the note's own.
    pub face: String,
    pub mono: String,
    /// The words' size in points. Code goes with it.
    pub size: f32,
    /// `tight`, `normal` or `loose`.
    pub spacing: String,
    /// The note's name over its words.
    pub name: bool,
    pub numbers: bool,
    pub contents: bool,
    /// Off, the page is ink on paper alone, for a printer with no color.
    pub color: bool,
    /// Each top heading starts a page.
    pub breaks: bool,
}

impl PdfConfig {
    pub const LEAST_SIZE: f32 = 8.;
    pub const MOST_SIZE: f32 = 16.;
}

impl Default for PdfConfig {
    fn default() -> Self {
        Self { paper: String::new(), landscape: false, margins: "normal".into(), face: String::new(), mono: String::new(), size: 10.5, spacing: "normal".into(), name: true, numbers: true, contents: false, color: true, breaks: false }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub app: AppConfig,
    pub editor: EditorConfig,
    pub pdf: PdfConfig,
}

impl Config {
    /// The settings as the app runs with them: the file, then the
    /// environment's overrides (`ONYX_APPEARANCE` for a probe).
    pub fn load() -> Config {
        let mut cfg = Self::on_disk();
        if let Ok(v) = std::env::var("ONYX_APPEARANCE") {
            if !v.is_empty() {
                cfg.app.appearance = v;
            }
        }
        cfg
    }

    fn on_disk() -> Config {
        paths::read_json(&paths::config_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    /// Change the file on disk. It starts from the file, not from `load`,
    /// so an override in the environment is never written back.
    pub fn edit(change: impl FnOnce(&mut Config)) -> std::io::Result<()> {
        let mut cfg = Self::on_disk();
        change(&mut cfg);
        paths::ensure_dirs()?;
        paths::write_json(&paths::config_file(), &serde_json::to_value(&cfg)?)
    }
}
