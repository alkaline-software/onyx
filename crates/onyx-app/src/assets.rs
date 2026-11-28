//! The pictures the window draws, over the set gpui-component ships.
//!
//! `Icon::new(IconName::…)` and `Icon::default().path(…)` resolve through
//! the app's `AssetSource`, and this one answers first for every file it
//! carries, then hands the rest to the toolkit's own set:
//!
//! - `icons/`: Tabler's outline icons (MIT, `TABLER-LICENSE` beside them),
//!   each under the name the toolkit asks for (`close.svg` is Tabler's
//!   `x`), so the icons the toolkit draws by itself are Tabler's too; the
//!   ones drawn here in Tabler's hand (`file-plus.svg`, `menu-bar.svg`,
//!   the flags); and the menu bar item's picture, the app icon in one ink, filled while
//!   its note is open (`menubar-note.svg`, `menubar-note-open.svg`,
//!   `sys/status_item.rs`).
//! - `icon/`: the app's own icon at each size `scripts/make-icon.sh` writes.
//! - `catppuccin/`: a file tree's icons (`file_icons.rs`).
//!
//! The folders are built in whole, so a file put there is served: no list
//! to keep beside them.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/**"]
#[include = "icon/*.png"]
#[include = "catppuccin/**"]
pub struct Own;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.starts_with("tabler/") {
            return Ok(crate::folder_look::svg(path).map(Cow::Owned));
        }
        if let Some(f) = Own::get(path) {
            return Ok(Some(f.data));
        }
        // A file icon at a size of its own (`file_icons::picture`).
        if let Some((base, points)) = path.rsplit_once('@').filter(|(b, _)| b.starts_with(crate::file_icons::PREFIX)) {
            let sized = points.parse().ok().zip(Own::get(base)).and_then(|(points, f)| crate::file_icons::sized_svg(&f.data, points));
            return Ok(sized.map(Cow::Owned));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut v = gpui_component_assets::Assets.list(path)?;
        v.extend(Own::iter().filter(|p| p.starts_with(path)).map(|p| SharedString::from(p.into_owned())));
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon the window names by path is carried: one that is not
    /// draws nothing, and nothing says so.
    #[test]
    fn every_named_icon_is_carried() {
        let sources = [include_str!("main.rs"), include_str!("widgets.rs"), include_str!("shell.rs"), include_str!("settings.rs"), include_str!("menu.rs"), include_str!("i18n.rs"), include_str!("editor.rs"), include_str!("tabs.rs"), include_str!("langs.rs"), include_str!("menubar.rs"), include_str!("note_menu.rs"), include_str!("folder_look.rs")];
        let mut missing = Vec::new();
        for src in sources {
            for (at, _) in src.match_indices("\"icons/") {
                let rest = &src[at + 1..];
                let path = &rest[..rest.find('"').unwrap_or(0)];
                if path.ends_with(".svg") && Own::get(path).is_none() {
                    missing.push(path.to_string());
                }
            }
        }
        assert!(missing.is_empty(), "not in assets/: {missing:?}");
    }

    #[test]
    fn the_app_icon_is_carried() {
        for size in ["icon/icon-128.png", "icon/icon-512.png"] {
            assert!(Own::get(size).is_some(), "{size}: run scripts/make-icon.sh");
        }
    }
}
