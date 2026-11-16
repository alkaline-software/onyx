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
    /// The colors the person made for folders, as `#rrggbb`, offered
    /// after the app's own in every vault.
    pub folder_colors: Vec<String>,
    /// The container: the one folder the vaults are in, each folder at
    /// its top a vault. Empty until it is chosen, or taken from the vault
    /// shown before there was one.
    pub container: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { appearance: "system".into(), accent: "teal".into(), language: "system".into(), check_updates: true, user_name: String::new(), avatar: String::new(), folder_colors: Vec::new(), container: String::new() }
    }
}

/// How a note is set in the editor and as it reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EditorConfig {
    /// The face a note's English (every script but Chinese, Japanese and
    /// Korean) is set in, by its key in the app's `fonts::SERIFS` or
    /// `fonts::SANS`, or `custom` for `text_custom`.
    pub text_face: String,
    /// The face code is set in, by its key in `fonts::MONOS`, or `custom`.
    pub mono_face: String,
    /// The face Chinese, Japanese and Korean are set in, by its key in
    /// `fonts::CJK` (`auto` matches the English face, a serif behind a
    /// serif), or `custom`.
    pub cjk_face: String,
    /// A font installed on the machine, by its family name, for each of
    /// the three: the face its `custom` stands for, kept while a carried
    /// face is chosen so the Custom tile still shows it.
    pub text_custom: String,
    pub mono_custom: String,
    pub cjk_custom: String,
    /// Whether `custom` is a key of its own. Before it was, a name in a
    /// custom field stood over the chosen face; such a file is read so
    /// (`Config::load`). Missing from a file, it is false: the struct's
    /// own default, a new install's, is true.
    #[serde(default)]
    pub custom_keyed: bool,
    /// The size of a note's words and of its code, in points.
    pub size: TextSizes,
    /// From before the presets went: `small`, `medium`, `large` or
    /// `custom`, and the sizes Custom had. Read once into `size` (`one_size`), never
    /// written.
    #[serde(skip_serializing)]
    text_size: String,
    #[serde(skip_serializing)]
    sizes: Option<TextSizes>,
    /// Whether a note's spelling and grammar are marked.
    pub check_writing: bool,
    /// The language they are checked in, by its key in `check::LANGUAGES`.
    pub writing_language: String,
}

impl EditorConfig {
    /// A file from before `custom` was a key: a name in a custom field
    /// stood over the chosen face, so its key is `custom` now.
    pub fn key_custom(&mut self) {
        self.one_size();
        if self.custom_keyed {
            return;
        }
        for (key, name) in [(&mut self.text_face, &self.text_custom), (&mut self.mono_face, &self.mono_custom), (&mut self.cjk_face, &self.cjk_custom)] {
            if !name.trim().is_empty() {
                *key = "custom".into();
            }
        }
        self.custom_keyed = true;
    }
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self { text_face: "inter".into(), mono_face: "jetbrains".into(), cjk_face: "auto".into(), text_custom: String::new(), mono_custom: String::new(), cjk_custom: String::new(), custom_keyed: true, size: TextSizes::MEDIUM, text_size: String::new(), sizes: None, check_writing: true, writing_language: "en-US".into() }
    }
}

impl EditorConfig {
    /// The sizes in force.
    pub fn text_sizes(&self) -> TextSizes {
        self.size.clamped()
    }

    /// A file from before the presets went: its preset's sizes, or
    /// Custom's.
    fn one_size(&mut self) {
        let was = std::mem::take(&mut self.text_size);
        let sizes = match was.as_str() {
            "" => return,
            "small" => TextSizes { note: 14., code: 13. },
            "large" => TextSizes { note: 18., code: 16. },
            "custom" => self.sizes.unwrap_or(TextSizes::MEDIUM),
            _ => TextSizes::MEDIUM,
        };
        self.size = sizes.clamped();
        self.sizes = None;
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
    /// The default: a note at Obsidian's own body size, code two points
    /// under, as a Mac's editors set monospace.
    pub const MEDIUM: Self = Self { note: 16., code: 14. };
    /// What a size may be set to.
    pub const LEAST: f32 = 9.;
    pub const MOST: f32 = 32.;

    pub fn clamped(self) -> Self {
        Self { note: self.note.clamp(Self::LEAST, Self::MOST), code: self.code.clamp(Self::LEAST, Self::MOST) }
    }

    /// A step up or down from `value`: a whole point.
    pub fn step(value: f32, up: bool) -> f32 {
        let to = if up { value.floor() + 1. } else { value.ceil() - 1. };
        to.clamp(Self::LEAST, Self::MOST)
    }
}

/// How a note is set as a PDF, as the export sheet last left it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PdfConfig {
    /// What the sheet makes: `pdf`, `image` (the note as one long PNG),
    /// `word` (.docx) or `html`.
    pub format: String,
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
    /// The files the note embeds or links to, after its words, each with
    /// a way there and back (`attach`).
    pub attached: bool,
}

impl PdfConfig {
    pub const LEAST_SIZE: f32 = 8.;
    pub const MOST_SIZE: f32 = 16.;

    /// Whether the note is set on one page as long as it is, with no
    /// paper's height: an image, and the web's page.
    pub fn long(&self) -> bool {
        matches!(self.format.as_str(), "image" | "html")
    }
}

impl Default for PdfConfig {
    fn default() -> Self {
        Self { format: "pdf".into(), paper: String::new(), landscape: false, margins: "normal".into(), face: String::new(), mono: String::new(), size: 10.5, spacing: "normal".into(), name: true, numbers: true, contents: false, color: true, breaks: false, attached: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub app: AppConfig,
    pub editor: EditorConfig,
    pub pdf: PdfConfig,
    pub terminal: TerminalConfig,
    pub menu_bar: MenuBarConfig,
}

/// The note in the menu bar, as Settings, Menu Bar changes it: whether
/// the item is there, and the shortcut that opens and closes the note from
/// any app (`Shortcut::to_config`'s text; empty for none).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MenuBarConfig {
    pub shown: bool,
    pub shortcut: String,
}

impl Default for MenuBarConfig {
    fn default() -> Self {
        Self { shown: true, shortcut: "ctrl-alt-cmd-o".into() }
    }
}

/// The terminal beside a note, as Settings, Terminal changes it. The look
/// is Kaku's, as Emaki's terminal has it; the letters' size, a row's
/// height and the room round the screen are fixed, and the face is the
/// window's code face.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TerminalConfig {
    /// `match` follows the window's appearance; `dark` and `light` keep
    /// Kaku Dark or Kaku Light whatever the window is.
    pub theme: String,
    /// `bar`, `block` or `underline`.
    pub cursor: String,
    pub cursor_blink: bool,
    /// Whether the face may join letters ("->" as an arrow).
    pub ligatures: bool,
    /// A selection is copied when the button is let go.
    pub copy_on_select: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self { theme: "match".into(), cursor: "bar".into(), cursor_blink: true, ligatures: false, copy_on_select: true }
    }
}

impl Config {
    /// The settings as the app runs with them: the file, then the
    /// environment's overrides (`ONYX_APPEARANCE` for a probe).
    pub fn load() -> Config {
        let mut cfg = Self::on_disk();
        cfg.editor.key_custom();
        if let Ok(v) = std::env::var("ONYX_APPEARANCE") {
            if !v.is_empty() {
                cfg.app.appearance = v;
            }
        }
        cfg
    }

    fn on_disk() -> Config {
        let mut cfg: Config = paths::read_json(&paths::config_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        cfg.editor.key_custom();
        cfg
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

#[cfg(test)]
mod custom_key_tests {
    use super::*;

    #[test]
    fn a_name_typed_before_custom_was_a_key_is_custom_now() {
        let old: EditorConfig = serde_json::from_value(serde_json::json!({ "text_face": "inter", "cjk_face": "auto", "cjk_custom": "TsangerJinKai02" })).unwrap();
        let mut e = old;
        e.key_custom();
        assert_eq!((e.text_face.as_str(), e.cjk_face.as_str()), ("inter", "custom"));
        // A file written since keeps a carried face chosen beside a kept name.
        let new: EditorConfig = serde_json::from_value(serde_json::json!({ "cjk_face": "wenkai", "cjk_custom": "TsangerJinKai02", "custom_keyed": true })).unwrap();
        let mut e = new;
        e.key_custom();
        assert_eq!(e.cjk_face, "wenkai");
        assert!(EditorConfig::default().custom_keyed);
    }
}
