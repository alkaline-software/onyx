//! A file tree's icons: Catppuccin's, as its VS Code icon theme chooses
//! them.
//!
//! `assets/catppuccin/` holds the theme's icons in two flavors (Latte for
//! a light window, Mocha for a dark one) and `theme.json`, its table of
//! which icon a name gets, both copied from the extension
//! (`catppuccin.catppuccin-vsc-icons` 1.26.0, MIT, `LICENSE` beside them).
//! The icons carry their own colors, so they are drawn as pictures
//! (`img`), not as the one-color masks the window's other icons are.
//!
//! An icon the theme lacks is drawn here in the theme's hand (a 16-pixel
//! line, round ends, a light fill) and added to the table under its
//! names. `quarto.svg` is one: Quarto's circle of four quarters, each
//! with a light fill, in Markdown's color, since a Quarto file is
//! markdown and more. A newer release of the theme is copied over the
//! folder, and these and their table lines are put back after it.

#[cfg(test)]
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

use gpui::SharedString;

/// Where the icons are served from, by `assets::Assets`.
pub const PREFIX: &str = "catppuccin/";

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Table {
    file: String,
    folder: String,
    folder_expanded: String,
    file_extensions: HashMap<String, String>,
    file_names: HashMap<String, String>,
    folder_names: HashMap<String, String>,
    folder_names_expanded: HashMap<String, String>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    let bytes = crate::assets::Own::get("catppuccin/theme.json").expect("the icon table is built in");
    serde_json::from_slice(&bytes.data).expect("the icon table reads")
});

#[cfg(test)]
fn load(path: &str) -> Option<Cow<'static, [u8]>> {
    crate::assets::Own::get(path).map(|f| f.data)
}

fn named<'a>(table: &'a HashMap<String, String>, name: &str) -> Option<&'a String> {
    table.get(name).or_else(|| table.get(&name.to_lowercase()))
}

/// The icon's id for a file of this name: the whole name first, then
/// its extensions from the longest ("d.ts" before "ts").
fn file_id(name: &str) -> &'static str {
    let t = &*TABLE;
    if let Some(id) = named(&t.file_names, name) {
        return id;
    }
    let mut rest = name;
    while let Some((_, ext)) = rest.split_once('.') {
        if let Some(id) = named(&t.file_extensions, ext) {
            return id;
        }
        rest = ext;
    }
    &t.file
}

fn folder_id(name: &str, open: bool) -> &'static str {
    let t = &*TABLE;
    let (names, plain) = if open { (&t.folder_names_expanded, &t.folder_expanded) } else { (&t.folder_names, &t.folder) };
    named(names, name).unwrap_or(plain)
}

/// The asset path of the icon a row of the tree wears.
pub fn path(name: &str, dir: bool, open: bool, dark: bool) -> SharedString {
    let id = if dir { folder_id(name, open) } else { file_id(name) };
    format!("{PREFIX}{}/{id}.svg", if dark { "mocha" } else { "latte" }).into()
}

/// The icon as a picture drawn `size` points wide, sharp at that size.
/// gpui reads an SVG picture once, at twice the size its file says, and
/// scales that copy to wherever it is drawn: a 16-point icon drawn at 72
/// on a phone's three pixels a point was a blur with its edges' pixels
/// smeared down its sides. So the file is served saying a size near the
/// drawn one (`sized_svg`, under `<path>@<points>`), a step of 8 points
/// at a time, enough for three pixels a point.
pub fn picture(name: &str, dir: bool, open: bool, dark: bool, size: f32) -> gpui::Img {
    let points = ((size * 1.5 / 8.).ceil() * 8.).max(16.) as u32;
    gpui::img(SharedString::from(format!("{}@{points}", path(name, dir, open, dark))))
}

/// An icon's SVG saying it is `points` wide and high, for `picture`.
pub fn sized_svg(svg: &[u8], points: u32) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(svg).ok()?;
    let start = text.find("<svg")?;
    let end = start + text[start..].find('>')?;
    let mut tag = text[start..end].to_string();
    for attr in ["width", "height"] {
        let key = format!(" {attr}=\"");
        if let Some(at) = tag.find(&key) {
            let from = at + key.len();
            let to = from + tag[from..].find('"')?;
            tag.replace_range(from..to, &points.to_string());
        } else {
            tag.push_str(&format!(" {attr}=\"{points}\""));
        }
    }
    Some(format!("{}{tag}{}", &text[..start], &text[end..]).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_get_the_icons_the_theme_gives_them() {
        assert_eq!(file_id("main.rs"), "rust");
        assert_eq!(file_id("Cargo.toml"), "cargo");
        assert_eq!(file_id("Cargo.lock"), "cargo-lock");
        assert_eq!(file_id("README.md"), "readme");
        assert_eq!(file_id("notes.md"), "markdown");
        assert_eq!(file_id("LICENSE"), "license");
        assert_eq!(file_id("no-such.zzzz"), "_file");
        // Drawn here, for what the theme has no icon for.
        assert_eq!(file_id("works.qmd"), "quarto");
        // YAML is YAML's whatever it is for, Quarto's own config too.
        assert_eq!(file_id("_quarto.yml"), "yaml");
        assert_eq!(folder_id(".quarto", false), "folder_quarto");
        assert_eq!(folder_id("R", false), "folder_r");
        assert_eq!(folder_id("target", false), "folder_dist");
        assert_eq!(folder_id("scripts", false), "folder_scripts");
        assert_eq!(folder_id("scripts", true), "folder_scripts_open");
        assert_eq!(folder_id("whatever", false), "_folder");
        // Every icon the table names is there, in both flavors.
        let t = &*TABLE;
        let ids = t.file_extensions.values().chain(t.file_names.values()).chain(t.folder_names.values()).chain(t.folder_names_expanded.values()).chain([&t.file, &t.folder, &t.folder_expanded]);
        for id in ids {
            for flavor in ["latte", "mocha"] {
                assert!(load(&format!("{PREFIX}{flavor}/{id}.svg")).is_some(), "{flavor}/{id}");
            }
        }
    }
}
