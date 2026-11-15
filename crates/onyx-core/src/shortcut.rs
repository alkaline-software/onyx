//! A shortcut: modifiers held and a key pressed with them, such as ⌃⌥←.
//!
//! Keys are the Mac's virtual key codes (the US layout's positions), the
//! numbers a key event and the system's hot keys both use, so a key keeps
//! its place whatever layout the person types in. Modifiers are the Mac's
//! event flags. The config keeps a shortcut as text ("ctrl-alt-left") so
//! it reads and can be edited by hand. Taken from Mado
//! (`mado/crates/mado-core/src/shortcut.rs`), which holds a shortcut for
//! every window action; Onyx holds the menu bar note's.

use std::fmt;

/// The Mac's modifier flags, as a key event reports them.
pub const CMD: u64 = 0x10_0000;
pub const SHIFT: u64 = 0x2_0000;
pub const CTRL: u64 = 0x4_0000;
pub const ALT: u64 = 0x8_0000;
/// The four that count; caps lock, fn and the side bits are ignored.
pub const MODS: u64 = CMD | SHIFT | CTRL | ALT;

pub const ESCAPE: u16 = 53;
pub const BACKSPACE: u16 = 51;
pub const FORWARD_DELETE: u16 = 117;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub mods: u64,
    pub key: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    /// A key with no ⌘, ⌥ or ⌃ is what typing sends: it would be taken
    /// from every app. A function key is the one key that may stand alone.
    NoModifier,
    UnknownKey,
}

impl Invalid {
    pub const ALL: [Invalid; 2] = [Invalid::NoModifier, Invalid::UnknownKey];

    /// What the window says of it, in English: the key of its other
    /// languages.
    pub fn said(&self) -> &'static str {
        match self {
            Invalid::NoModifier => "Hold ⌘, ⌥ or ⌃ with the key",
            Invalid::UnknownKey => "That key can't be used",
        }
    }
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.said())
    }
}

impl Shortcut {
    /// A shortcut from what was pressed, or why it cannot be one.
    pub fn new(flags: u64, key: u16) -> Result<Self, Invalid> {
        let mods = flags & MODS;
        let name = key_name(key).ok_or(Invalid::UnknownKey)?;
        if mods & (CMD | ALT | CTRL) == 0 && !is_function_key(name) {
            return Err(Invalid::NoModifier);
        }
        Ok(Self { mods, key })
    }

    /// "ctrl-alt-left", "ctrl-alt-cmd-right", "ctrl-alt--": what the
    /// config keeps.
    pub fn to_config(&self) -> String {
        let mut parts: Vec<&str> = MOD_NAMES.iter().filter(|(bit, _, _)| self.mods & bit != 0).map(|(_, name, _)| *name).collect();
        parts.push(key_name(self.key).unwrap_or("?"));
        parts.join("-")
    }

    pub fn from_config(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        // The key may itself be "-", as in "ctrl-alt--".
        let (mods, key) = match text.strip_suffix("--") {
            Some(rest) => (rest, "-"),
            None => text.rsplit_once('-').unwrap_or(("", text.as_str())),
        };
        let mut bits = 0;
        for part in mods.split('-').filter(|p| !p.is_empty()) {
            bits |= MOD_NAMES.iter().find(|(_, name, _)| *name == part || alias(part) == Some(*name))?.0;
        }
        Shortcut::new(bits, key_code(key)?).ok()
    }

    /// Each key on a cap of its own, in the order `Display` writes them:
    /// ["⌃", "⌥", "←"].
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = MOD_NAMES.iter().filter(|(bit, _, _)| self.mods & bit != 0).map(|(_, _, s)| s.to_string()).collect();
        keys.push(key_label(self.key));
        keys
    }
}

impl fmt::Display for Shortcut {
    /// "⌃⌥←", "⌃⌥⌘→", "⌃⌥Space": the Mac's signs in the Mac's order, then
    /// the key, as a menu writes it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let signs: String = MOD_NAMES.iter().filter(|(bit, _, _)| self.mods & bit != 0).map(|(_, _, s)| *s).collect();
        write!(f, "{signs}{}", key_label(self.key))
    }
}

/// In the order the Mac writes them: ⌃⌥⇧⌘.
const MOD_NAMES: [(u64, &str, &str); 4] = [(CTRL, "ctrl", "⌃"), (ALT, "alt", "⌥"), (SHIFT, "shift", "⇧"), (CMD, "cmd", "⌘")];

fn alias(name: &str) -> Option<&'static str> {
    match name {
        "command" | "super" | "secondary" => Some("cmd"),
        "option" | "opt" => Some("alt"),
        "control" => Some("ctrl"),
        _ => None,
    }
}

fn is_function_key(name: &str) -> bool {
    name.len() >= 2 && name.starts_with('f') && name[1..].chars().all(|c| c.is_ascii_digit())
}

/// Key codes and the names the config uses, US layout.
#[rustfmt::skip]
const KEYS: &[(u16, &str)] = &[
    (0, "a"), (11, "b"), (8, "c"), (2, "d"), (14, "e"), (3, "f"), (5, "g"), (4, "h"), (34, "i"), (38, "j"),
    (40, "k"), (37, "l"), (46, "m"), (45, "n"), (31, "o"), (35, "p"), (12, "q"), (15, "r"), (1, "s"), (17, "t"),
    (32, "u"), (9, "v"), (13, "w"), (7, "x"), (16, "y"), (6, "z"),
    (29, "0"), (18, "1"), (19, "2"), (20, "3"), (21, "4"), (23, "5"), (22, "6"), (26, "7"), (28, "8"), (25, "9"),
    (50, "`"), (27, "-"), (24, "="), (33, "["), (30, "]"), (42, "\\"), (41, ";"), (39, "'"), (43, ","), (47, "."), (44, "/"),
    (48, "tab"), (49, "space"), (BACKSPACE, "backspace"), (FORWARD_DELETE, "delete"), (ESCAPE, "escape"), (36, "enter"), (76, "keypad-enter"),
    (123, "left"), (124, "right"), (125, "down"), (126, "up"),
    (115, "home"), (119, "end"), (116, "pageup"), (121, "pagedown"),
    (122, "f1"), (120, "f2"), (99, "f3"), (118, "f4"), (96, "f5"), (97, "f6"),
    (98, "f7"), (100, "f8"), (101, "f9"), (109, "f10"), (103, "f11"), (111, "f12"),
    (105, "f13"), (107, "f14"), (113, "f15"), (106, "f16"), (64, "f17"), (79, "f18"), (80, "f19"),
];

pub fn key_name(code: u16) -> Option<&'static str> {
    KEYS.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

pub fn key_code(name: &str) -> Option<u16> {
    KEYS.iter().find(|(_, n)| *n == name).map(|(c, _)| *c)
}

/// How a key is written on screen: "←", "Space", "A", "F5", "⌫".
pub fn key_label(code: u16) -> String {
    match key_name(code) {
        Some("left") => "←".into(),
        Some("right") => "→".into(),
        Some("up") => "↑".into(),
        Some("down") => "↓".into(),
        Some("tab") => "⇥".into(),
        Some("space") => "Space".into(),
        Some("backspace") => "⌫".into(),
        Some("delete") => "⌦".into(),
        Some("escape") => "Esc".into(),
        Some("enter") | Some("keypad-enter") => "↩".into(),
        Some("home") => "↖".into(),
        Some("end") => "↘".into(),
        Some("pageup") => "⇞".into(),
        Some("pagedown") => "⇟".into(),
        Some(name) => name.to_uppercase(),
        None => format!("Key {code}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_config() {
        for text in ["ctrl-alt-left", "ctrl-alt-cmd-right", "ctrl-alt--", "ctrl-alt-=", "ctrl-alt-enter", "ctrl-alt-shift-up", "cmd-f5", "f13"] {
            let s = Shortcut::from_config(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(s.to_config(), text);
        }
        assert_eq!(Shortcut::from_config("Control-Option-Left").unwrap(), Shortcut { mods: CTRL | ALT, key: 123 });
    }

    #[test]
    fn rejects_what_would_take_typing() {
        assert_eq!(Shortcut::new(0, 0), Err(Invalid::NoModifier));
        assert_eq!(Shortcut::new(SHIFT, 0), Err(Invalid::NoModifier));
        assert_eq!(Shortcut::new(CTRL, 999), Err(Invalid::UnknownKey));
        assert!(Shortcut::new(0, 105).is_ok(), "a function key may stand alone");
        assert!(Shortcut::from_config("left").is_none());
        assert!(Shortcut::from_config("hyper-left").is_none());
        // Caps lock and the side bits are ignored.
        assert_eq!(Shortcut::new(ALT | 0x1_0000, 123), Ok(Shortcut { mods: ALT, key: 123 }));
    }

    #[test]
    fn writes_signs_in_mac_order() {
        assert_eq!(Shortcut { mods: CMD | CTRL | ALT, key: 124 }.to_string(), "⌃⌥⌘→");
        assert_eq!(Shortcut { mods: CTRL | ALT, key: 49 }.to_string(), "⌃⌥Space");
        assert_eq!(Shortcut { mods: ALT | SHIFT, key: 96 }.to_string(), "⌥⇧F5");
    }
}
