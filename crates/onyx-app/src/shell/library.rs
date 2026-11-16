//! The vault's assets (`vault::assets`, the dotted folder at its top):
//! the sidebar's second card, under the notes, and a file of it shown
//! in a tab of its own beside the notes.
//!
//! The sidebar is the vault's card, after Emaki's computers
//! (`render_side_stack`): the tabs of Notes and Assets at its top, the
//! vault's own row over the tree, and the vault's button at the foot. The
//! assets' card lists every file of the folder
//! by its kind (`preview::kind`), a row each: a picture's with its small
//! square in the icon's place, on a taller row. A press opens the file in a
//! tab (`open_asset`), drawn as a note is wherever it reads as markdown,
//! and as itself where it is a picture, a PDF, a video or a sound.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use onyx_core::preview::{self, Kind};
use onyx_core::vault;

use super::{edge_fade, page_ground, Shell, Tab, ROW_BREATH, TREE_ROW_H};
use crate::editor::{AssetDrag, Editor, Mode, Seen, Viewing};
use crate::embed::{Embed, TabAsk};
use crate::i18n::t;
use crate::menu::Entry;
use crate::widgets::{small_pill, swallow_click, Press, FOLD_ANIM};

/// A picture's row: taller than a file's, so its small square is large
/// enough to tell one picture from another.
const PICTURE_ROW_H: f32 = 47.;
/// A kind's name over its rows in the Assets card.
const LABEL_H: f32 = 34.;
const THUMB: f32 = 34.;
/// The pixels a square is drawn from: three times its size, for a Retina screen and a little more.
const THUMB_PX: u32 = 102;
/// How long the player waits to show: the page's fade, which the
/// system's view, drawn over the window, cannot take part in.
const PLAYER_WAIT: std::time::Duration = std::time::Duration::from_millis(200);
/// The room an asset keeps from the page's edges.
const ASSET_PAD: f32 = 24.;

/// Which card the sidebar has open.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum SideView {
    #[default]
    Notes,
    Assets,
}

impl SideView {
    pub(crate) fn key(self) -> &'static str {
        match self {
            SideView::Notes => "notes",
            SideView::Assets => "assets",
        }
    }

    pub(crate) fn from_key(key: &str) -> Self {
        if key == "assets" { SideView::Assets } else { SideView::Notes }
    }
}

/// The assets as the card lists them, read off the main thread whenever
/// the folder changes, and each picture's small square once it is drawn.
#[derive(Default)]
pub(crate) struct Library {
    pub files: Vec<(PathBuf, Kind)>,
    /// A picture's square: `None` while it is being drawn or could not be.
    pub thumbs: HashMap<PathBuf, Option<Arc<RenderImage>>>,
    pub scroll: ScrollHandle,
    /// An SVG's square, drawn by the card and let go of with the vault's
    /// assets, not kept by gpui (`pictures.rs`).
    pub kept: Option<Entity<crate::pictures::Kept>>,
}

/// What a tab of an asset holds besides its editor.
pub(crate) struct Asset {
    body: Body,
    /// When it was opened: the player waits for the page's fade.
    opened: Instant,
    /// What its frame asks of the window, heard.
    _sub: Option<Subscription>,
}

impl Asset {
    /// Whether the tab's editor draws it, as a note's page does.
    pub(crate) fn reads_as_words(&self) -> bool {
        matches!(self.body, Body::Words)
    }

    /// Whether it stands in a frame, with the frame's bar.
    pub(crate) fn framed(&self) -> bool {
        matches!(self.body, Body::Frame(_))
    }

    /// Its file renamed or moved: the frame follows.
    pub(crate) fn moved_to(&self, to: PathBuf, cx: &mut App) {
        if let Body::Frame(frame) = &self.body {
            frame.update(cx, |m, cx| m.moved_to(to, cx));
        }
    }
}

enum Body {
    /// Still being read off the main thread.
    Reading,
    /// Read as markdown, in the tab's editor.
    Words,
    /// A picture or a PDF, in the frame a note shows it in, with its bar
    /// over it (`embed.rs`, `Embed::for_tab`): the bar's zoom, View or the
    /// sidebar, and the file's menu.
    Frame(Entity<Embed>),
    /// A video or a sound in the system's player.
    Play(crate::media::Player),
    /// What is not drawn here, and why, in the window's words.
    Not(String),
}

/// Pixels in RGBA as the window paints them: gpui's images are BGRA.
fn render_image(width: u32, height: u32, mut rgba: Vec<u8>) -> Option<Arc<RenderImage>> {
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, rgba)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// A picture's small square, cut from its middle, drawn off the window's
/// thread: read no larger than its shorter side needs to fill the square
/// (`sys::picture_at_most`), never whole.
fn thumb(path: &Path) -> Option<Arc<RenderImage>> {
    let most = image::image_dimensions(path).ok().map_or(THUMB_PX * 4, |(w, h)| {
        let (long, short) = (w.max(h) as f32, w.min(h).max(1) as f32);
        ((THUMB_PX as f32 * long / short).ceil() as u32).clamp(THUMB_PX, THUMB_PX * 16)
    });
    let (w, h, rgba) = crate::sys::picture_at_most(path, most)?;
    let picture = image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, rgba)?);
    let side = picture.width().min(picture.height());
    let square = picture.crop_imm((picture.width() - side) / 2, (picture.height() - side) / 2, side, side);
    let small = square.thumbnail(THUMB_PX, THUMB_PX).into_rgba8();
    let (w, h) = small.dimensions();
    render_image(w, h, small.into_raw())
}

fn is_svg(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn kind_title(kind: Kind) -> &'static str {
    match kind {
        Kind::Picture => t("Pictures"),
        Kind::Document => t("Documents"),
        Kind::Media => t("Media"),
        Kind::Other => t("Other"),
    }
}

/// Something that fades in the first time it is drawn under this id: a
/// square or a page that has just been drawn.
fn fade_keyed(id: String, el: impl IntoElement) -> impl IntoElement {
    div().size_full().child(el).with_animation(ElementId::Name(id.into()), Animation::new(std::time::Duration::from_millis(220)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
}

impl Shell {
    /// A tab's name: a note's without its extension, as the tree has it;
    /// any other file's whole, so a PDF and a picture of one name differ.
    pub(crate) fn tab_title(path: &Path) -> String {
        if vault::is_note(path) { vault::title(path) } else { name_of(path) }
    }

    /// Whether a path is a file of the vault's assets.
    pub(crate) fn is_asset(&self, path: &Path) -> bool {
        self.vault_root().is_some_and(|r| path.starts_with(vault::assets(r))) && !path.is_dir()
    }

    // -- the list -----------------------------------------------------------

    /// The assets read again, off the main thread; the squares of
    /// pictures still there are kept.
    pub(crate) fn reread_assets(&mut self, cx: &mut Context<Self>) {
        if self.library.kept.is_none() {
            self.library.kept = Some(crate::pictures::Kept::new(cx));
        }
        let Some(root) = self.vault_root().map(Path::to_path_buf) else {
            self.library.files.clear();
            return;
        };
        let task = cx.background_spawn(async move {
            vault::asset_files(&root)
                .into_iter()
                .map(|p| {
                    let k = preview::kind(&p);
                    (p, k)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let files = task.await;
            let _ = this.update(cx, |s, cx| {
                s.library.thumbs.retain(|p, _| files.iter().any(|(f, _)| f == p));
                s.library.files = files;
                s.draw_thumbs(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Each picture's square not drawn yet, drawn off the main thread, two
    /// at a time (`layout::DECODING`), and only while the Assets card is
    /// open: a vault opened on its notes reads none of its pictures.
    fn draw_thumbs(&mut self, cx: &mut Context<Self>) {
        if self.side_view != SideView::Assets {
            return;
        }
        let wanted: Vec<PathBuf> = self.library.files.iter().filter(|(p, k)| *k == Kind::Picture && !is_svg(p) && !self.library.thumbs.contains_key(p)).map(|(p, _)| p.clone()).collect();
        for path in wanted {
            self.library.thumbs.insert(path.clone(), None);
            let task = cx.background_spawn({
                let path = path.clone();
                async move {
                    let _turn = crate::layout::DECODING.acquire().await;
                    thumb(&path)
                }
            });
            cx.spawn(async move |this, cx| {
                let drawn = task.await;
                let _ = this.update(cx, |s, cx| {
                    if s.library.thumbs.contains_key(&path) {
                        s.library.thumbs.insert(path, drawn);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    /// Whether a change on the disk is one in the assets folder.
    pub(crate) fn assets_changed(root: &Path, paths: &[PathBuf]) -> bool {
        let assets = vault::assets(root);
        paths.iter().any(|p| p.starts_with(&assets) && p != &assets.join(".DS_Store"))
    }

    pub(crate) fn set_side_view(&mut self, view: SideView, window: &mut Window, cx: &mut Context<Self>) {
        if self.side_view != view {
            self.side_moved = Some((self.side_view, Instant::now()));
            self.side_view = view;
            if view == SideView::Assets {
                self.reread_assets(cx);
            }
            self.save_ui(window);
            cx.notify();
        }
    }

    // -- the stack ----------------------------------------------------------

    /// The vault's card, and under it the vault's block, which lists the
    /// vaults (`render_vault_block`). The card's top is a pair of tabs, Notes and
    /// Assets, each half the width, after a design of Emaki's: the one
    /// open stands on a plate, which slides to the other at a press, and
    /// the card's content fades in under it. The tabs stand on a tinted
    /// band, its top rounded as the card's, since the card's own corner
    /// does not cut what is drawn in it.
    pub(crate) fn render_side_stack(&self, content: Div, ground: Hsla, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let moved = self.side_moved.filter(|(_, at)| at.elapsed() < FOLD_ANIM);
        let serial = self.side_view.key();
        let fading = moved.is_some();
        let fade_in = move |d: Div, id: String| d.with_animation(ElementId::Name(id.into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| if fading { d.opacity(0.35 + 0.65 * t) } else { d });
        let track_bg = if theme.mode.is_dark() { theme.foreground.opacity(0.035) } else { theme.foreground.opacity(0.05) };
        let plate_bg = if theme.mode.is_dark() { theme.foreground.opacity(0.09) } else { theme.background };
        // The plate: under the tab open, or on its way there from the other.
        let at = |v: SideView| if v == SideView::Notes { 0. } else { 0.5 };
        let to = at(self.side_view);
        let plate = div().absolute().top_0().bottom_0().w(relative(0.5)).rounded(px(7.)).bg(plate_bg).when(!theme.mode.is_dark(), |d| d.shadow_xs());
        let plate: AnyElement = match moved {
            Some((from, _)) => {
                let from = at(from);
                plate.with_animation(ElementId::Name(format!("side-plate-{serial}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.left(relative(from + (to - from) * t))).into_any_element()
            }
            None => plate.left(relative(to)).into_any_element(),
        };
        let tab = |view: SideView| {
            let (name, icon) = if view == SideView::Notes { (t("Notes"), "icons/file-text.svg") } else { (t("Assets"), "icons/image.svg") };
            let open = view == self.side_view;
            let ink = if open { theme.foreground } else { theme.muted_foreground };
            let hover = theme.foreground;
            h_flex()
                .id(ElementId::Name(format!("side-tab-{}", view.key()).into()))
                .flex_1()
                .min_w_0()
                .h_full()
                .justify_center()
                .items_center()
                .gap(px(7.))
                .text_size(px(12.5))
                .font_weight(if open { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .text_color(ink)
                .when(!open, |d| d.cursor_pointer().hover(move |s| s.text_color(hover)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.set_side_view(view, window, cx);
                }))
                .child(Icon::default().path(icon).with_size(px(14.)))
                .child(div().min_w_0().truncate().child(name))
        };
        let tabs = div().relative().h(px(30.)).child(plate).child(h_flex().size_full().child(tab(SideView::Notes)).child(tab(SideView::Assets)));
        let band = div().flex_shrink_0().p(px(4.)).rounded_t(px(9.)).bg(track_bg).border_b_1().border_color(theme.foreground.opacity(0.1)).child(tabs);
        let content = fade_in(content.flex_1().min_h_0(), format!("side-card-{serial}"));
        let card = v_flex().mx(px(8.)).mt(px(10.)).mb(px(8.)).flex_1().min_h_0().rounded(px(10.)).border_1().border_color(theme.foreground.opacity(0.1)).bg(ground).overflow_hidden().child(band).child(content);
        let block = self.render_vault_block(theme, cx);
        v_flex().flex_1().min_h_0().child(card).children(block).into_any_element()
    }

    /// The vault under its card (`vault_block`, as the menu bar note has
    /// it on its head): a press lists the vaults over it, as wide as it
    /// (`show_vaults_at`), and a press while they show puts them away; a
    /// right click gives the vault's menu (Rename, Icon and Color…, Reveal
    /// in Finder); a row let go on it goes to the vault's top.
    pub(crate) fn render_vault_block(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let root = self.vault_root()?.to_path_buf();
        let name = self.vault.as_ref().map(|v| v.tree.name.clone()).unwrap_or_default();
        let listed = self.menus.open.as_ref().is_some_and(|m| m.what == "vaults");
        let place = self.vaults_at.clone();
        let wash = theme.primary.opacity(0.22);
        let under = cx.theme().title_bar;
        let block = vault_block("vault-block", &root, &name, BlockSize::SIDEBAR, under, listed, theme, cx);
        Some(
            block
                .mx(px(8.))
                .mb(px(10.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    if this.menus.open.as_ref().is_some_and(|m| m.what == "vaults") {
                        this.menus.close(cx);
                        cx.notify();
                        return;
                    }
                    let b = this.vaults_at.get();
                    this.show_vaults_at(point(b.origin.x, b.origin.y - px(6.)), Some(b.size.width), true, cx);
                }))
                .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    swallow_click(window, cx);
                    this.show_vault_menu(ev.position, cx);
                }))
                .drag_over::<super::TreeDrag>(move |style, _, _, _| style.bg(wash))
                .on_drop(cx.listener(move |this, drag: &super::TreeDrag, window, cx| {
                    cx.stop_propagation();
                    this.tree_drop(drag, root.clone(), window, cx)
                }))
                // Where the list drops from: the block as it was last drawn.
                .child(canvas(move |b, _, _| place.set(b), |_, _, _, _| ()).absolute().top_0().left_0().size_full())
                .into_any_element(),
        )
    }

    /// The vault at the top of its tree (`vault_root_row`), with the
    /// Notes tools at its right: Collapse All Folders, and New Note and
    /// New Folder at the vault's top (⌘N makes beside the note showing).
    /// A right click gives the vault's menu; a row let go on it goes to
    /// the vault's top.
    pub(crate) fn render_vault_root(&self, ground: Hsla, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let root = self.vault_root()?.to_path_buf();
        let name = self.vault.as_ref().map(|v| v.tree.name.clone()).unwrap_or_default();
        let [collapse, note, folder] = notes_tools("side", theme);
        let wash = theme.primary.opacity(0.22);
        let tools = h_flex()
            .flex_shrink_0()
            .gap(px(2.))
            .child(collapse.on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.collapse_all(window, cx);
            })))
            .child(note.on_click(cx.listener({
                let root = root.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.new_note_in(root.clone(), window, cx);
                }
            })))
            .child(folder.on_click(cx.listener({
                let root = root.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.new_folder_in(root.clone(), window, cx);
                }
            })));
        Some(
            vault_root_row("side-vault-root", &root, &name, ground, theme, cx)
                .mx(px(8.))
                .mt(px(4.))
                .pr(px(4.))
                .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    swallow_click(window, cx);
                    this.show_vault_menu(ev.position, cx);
                }))
                .drag_over::<super::TreeDrag>(move |style, _, _, _| style.bg(wash))
                .on_drop(cx.listener(move |this, drag: &super::TreeDrag, window, cx| {
                    cx.stop_propagation();
                    this.tree_drop(drag, root.clone(), window, cx)
                }))
                .child(tools)
                .into_any_element(),
        )
    }

    /// The vaults' list (`vault_list`), the open one ticked; then New
    /// Vault…, made in the container, and Choose Container….
    pub(crate) fn show_vaults_at(&mut self, at: Point<Pixels>, width: Option<Pixels>, above: bool, cx: &mut Context<Self>) {
        let open = self.vault_root().map(Path::to_path_buf);
        let mut entries: Vec<Entry> = vault_list(self.container().as_deref(), open.as_deref())
            .into_iter()
            .map(|p| {
                let chosen = open.as_ref() == Some(&p);
                Entry::Choice { id: p.to_string_lossy().to_string().into(), icon: "icons/book-open.svg", label: vault_name(&p).into(), picture: false, chosen }
            })
            .collect();
        if !entries.is_empty() {
            entries.push(Entry::Rule);
        }
        // A vault is made in the container: with none, the way to one.
        if self.container().is_some() {
            entries.push(Entry::choice("new", "icons/plus.svg", t("New Vault…")));
        } else {
            entries.push(Entry::choice("container", "icons/folder-open.svg", t("Choose Container…")));
        }
        if open.is_some() {
            entries.push(Entry::choice("close", "icons/close.svg", t("Close Vault")));
        }
        if above {
            self.menus.show_above("vaults", at, width, entries);
        } else {
            self.menus.show("vaults", at, entries);
            if let Some(m) = self.menus.open.as_mut() {
                m.width = width;
            }
        }
        cx.notify();
    }

    /// The container chosen (`AppConfig::container`), while it is there.
    pub(crate) fn container(&self) -> Option<PathBuf> {
        container_of(&self.cfg)
    }

    // -- the assets' card ---------------------------------------------------

    /// The assets' card: a group of rows for each kind, pictures first,
    /// each under its name; with none, one line.
    pub(crate) fn render_assets(&self, theme: &gpui_component::Theme, ground: Hsla, cx: &mut Context<Self>) -> AnyElement {
        self.scrollers.name(&self.library.scroll);
        // Only the rows in view, and a little past, are drawn, as the
        // tree's (`render_tree`): the rest stand as room of their height.
        let view = f32::from(self.library.scroll.bounds().size.height);
        let top = -f32::from(self.library.scroll.offset().y);
        let (from, to) = if view > 0. { (top - super::TREE_OVER, top + view + super::TREE_OVER) } else { (0., super::TREE_FIRST) };
        let (mut y, mut gap) = (0., 0.);
        let mut sections: Vec<AnyElement> = Vec::new();
        for kind in [Kind::Picture, Kind::Document, Kind::Media, Kind::Other] {
            let count = self.library.files.iter().filter(|(_, k)| *k == kind).count();
            if count == 0 {
                continue;
            }
            let label = div().h(px(LABEL_H)).flex_shrink_0().px(px(6.)).pt(px(10.)).pb(px(6.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(format!("{}  {}", kind_title(kind), count));
            let row_h = if kind == Kind::Picture { PICTURE_ROW_H } else { TREE_ROW_H };
            let mut rows: Vec<AnyElement> = Vec::new();
            y += LABEL_H;
            for (p, _) in self.library.files.iter().filter(|(_, k)| *k == kind) {
                if y + row_h > from && y < to {
                    if gap > 0. {
                        rows.push(div().h(px(gap)).flex_shrink_0().into_any_element());
                        gap = 0.;
                    }
                    rows.push(self.render_asset_row(p, kind == Kind::Picture, theme, cx));
                } else {
                    gap += row_h;
                }
                y += row_h;
            }
            if gap > 0. {
                rows.push(div().h(px(gap)).flex_shrink_0().into_any_element());
                gap = 0.;
            }
            sections.push(v_flex().child(label).child(v_flex().children(rows)).into_any_element());
        }
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                v_flex()
                    .id("assets-list")
                    .size_full()
                    .px(px(8.))
                    .pb(px(12.))
                    .overflow_y_scroll()
                    .track_scroll(&self.library.scroll)
                    .children(sections),
            )
            .child(edge_fade(&self.library.scroll, ground, true, px(0.)))
            .child(edge_fade(&self.library.scroll, ground, false, px(9.)))
            .vertical_scrollbar(&self.library.scroll)
            .into_any_element()
    }

    /// A file's row: its icon and its whole name, or for a picture its
    /// small square in the icon's place on a taller row.
    fn render_asset_row(&self, path: &Path, picture: bool, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let name = name_of(path);
        let chosen = self.shown.as_deref() == Some(path);
        let mark: AnyElement = if !picture {
            img(crate::file_icons::path(&name, false, false, true)).size(px(16.)).flex_shrink_0().into_any_element()
        } else {
            let square: AnyElement = if let Some(kept) = self.library.kept.as_ref().filter(|_| is_svg(path)) {
                img(crate::pictures::through(kept, ImageSource::Resource(Resource::Path(path.to_path_buf().into())), THUMB_PX)).size_full().object_fit(ObjectFit::Contain).into_any_element()
            } else if is_svg(path) {
                div().into_any_element()
            } else {
                match self.library.thumbs.get(path).cloned().flatten() {
                    Some(drawn) => fade_keyed(format!("thumb-{}", path.display()), img(drawn).size_full().object_fit(ObjectFit::Cover)).into_any_element(),
                    None => div().into_any_element(),
                }
            };
            div().size(px(THUMB)).flex_shrink_0().rounded(px(6.)).overflow_hidden().bg(theme.foreground.opacity(0.06)).border_1().border_color(theme.foreground.opacity(0.08)).child(square).into_any_element()
        };
        let (open, menu) = (path.to_path_buf(), path.to_path_buf());
        // As the tree's rows: the row takes the press, its plate is lit a
        // little in from its top and foot.
        let hover_bg = theme.foreground.opacity(0.07);
        let plate = h_flex()
            .size_full()
            .px(px(if picture { 5. } else { 8. }))
            .gap(px(if picture { 8. } else { 6. }))
            .items_center()
            .rounded(px(7.))
            .when(chosen, |d| d.bg(theme.foreground.opacity(0.13)))
            .when(!chosen, |d| d.group_hover("asset-row", move |s| s.bg(hover_bg)))
            .when(self.pressed.as_deref() == Some(path), |d| d.bg(theme.foreground.opacity(0.1)))
            .child(mark)
            .child(div().flex_1().min_w_0().truncate().child(name.clone()));
        div()
            .id(ElementId::Name(format!("asset-row-{}", path.display()).into()))
            .group("asset-row")
            .h(px(if picture { PICTURE_ROW_H } else { TREE_ROW_H }))
            .flex_shrink_0()
            .py(px(ROW_BREATH))
            .text_size(px(13.))
            .text_color(theme.foreground.opacity(if chosen { 1. } else { 0.88 }))
            .cursor_pointer()
            .on_drag(AssetDrag(path.to_path_buf(), name.into()), |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                this.open_asset(open.clone(), window, cx);
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                swallow_click(window, cx);
                this.show_asset_menu(menu.clone(), ev.position, false, cx);
            }))
            .child(plate)
            .into_any_element()
    }

    /// An asset's menu, in the order every menu here keeps: its name,
    /// what opens or shows it, what copies it, and the trash last. In the
    /// file's own tab (`in_tab`, its frame's bar) it is open already.
    pub(crate) fn show_asset_menu(&mut self, path: PathBuf, at: Point<Pixels>, in_tab: bool, cx: &mut Context<Self>) {
        let mut entries = vec![Entry::choice("rename", "icons/pencil-simple.svg", t("Rename")), Entry::Rule];
        if !in_tab {
            entries.push(Entry::choice("open", "icons/eye.svg", t("Open")));
        }
        entries.extend([
            Entry::choice("open-app", "icons/external-link.svg", t("Open with Default App")),
            Entry::choice("reveal", "icons/folder-open.svg", t(crate::sys::REVEAL_LABEL)),
            Entry::Rule,
            Entry::choice("copy-link", "icons/link.svg", t("Copy Link")),
            Entry::choice("copy-path", "icons/link.svg", t("Copy Path")),
            Entry::Rule,
            Entry::choice("trash", "icons/trash.svg", t("Move to Trash")),
        ]);
        self.pressed = Some(path);
        self.menus.show("asset", at, entries);
        cx.notify();
    }

    /// A choice on an asset's menu.
    pub(crate) fn asset_pick(&mut self, id: &str, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "rename" => self.start_rename(path, window, cx),
            "open" => self.open_asset(path, window, cx),
            "open-app" => self.open_outside(&path.to_string_lossy()),
            "reveal" => crate::sys::reveal(&path),
            // The link a note would write for it, from the note showing.
            "copy-link" => {
                let from = self.shown.clone().filter(|p| vault::is_note(p)).or_else(|| self.vault_root().map(|r| r.join("note.md"))).unwrap_or_default();
                crate::widgets::to_clipboard(vault::reference(&from, &path), cx);
            }
            "copy-path" => crate::widgets::to_clipboard(path.to_string_lossy().to_string(), cx),
            "trash" => {
                self.trash(path, window, cx);
                self.reread_assets(cx);
            }
            _ => {}
        }
    }

    // -- an asset's tab ------------------------------------------------------

    /// A file of the vault's assets in a tab of its own, read off the main
    /// thread and then drawn as its kind is; a tab already on it shows.
    pub(crate) fn open_asset(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.iter().any(|t| t.path == path) {
            self.show_tab(&path, window, cx);
            return;
        }
        // Its editor draws it when it reads as markdown: as it reads,
        // with nothing to type in, its links followed as a note's are.
        let editor = cx.new(Editor::new);
        let (look, root) = (self.look.clone(), self.vault_root().map(Path::to_path_buf));
        editor.update(cx, |e, cx| {
            e.set_look(look, cx);
            e.set_mode(Mode::Reading, cx);
            e.set_place(path.clone(), root, cx);
        });
        let sub = Self::listen(&editor, path.clone(), cx);
        let (title, title_sub) = self.make_title(&path, window, cx);
        let asset = Asset { body: Body::Reading, opened: Instant::now(), _sub: None };
        self.tabs.push(Tab { path: path.clone(), editor, title, dirty: false, saved: String::new(), save_due: None, _sub: sub, _title_sub: title_sub, asset: Some(asset), source: None });
        self.closed_tabs.retain(|c| *c != path);
        self.sort_tabs();
        self.show_tab(&path, window, cx);
        let read = cx.background_spawn({
            let path = path.clone();
            async move {
                let read = preview::read(&path);
                let size = matches!(read, preview::Preview::Picture).then(|| image::image_dimensions(&path).ok()).flatten();
                let sizes = if matches!(read, preview::Preview::Pdf { .. }) { preview::pdf_sizes(&path).unwrap_or_default() } else { Vec::new() };
                (read, size, sizes)
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let (read, size, sizes) = read.await;
            let _ = this.update_in(cx, |s, window, cx| {
                let Some(ix) = s.tabs.iter().position(|t| t.path == path && t.asset.is_some()) else { return };
                let mut sub = None;
                let body = match read {
                    preview::Preview::Markdown { text, cut } => {
                        let text = if cut { format!("{text}\n\n*{}*\n", t("Only the start of the file is shown.")) } else { text };
                        s.tabs[ix].editor.update(cx, |e, cx| e.set_text(&text, cx));
                        Body::Words
                    }
                    preview::Preview::Picture => {
                        let (frame, heard) = s.asset_frame(ix, None, size, cx);
                        sub = Some(heard);
                        Body::Frame(frame)
                    }
                    preview::Preview::Pdf { .. } if !sizes.is_empty() => {
                        let (frame, heard) = s.asset_frame(ix, Some(sizes), None, cx);
                        sub = Some(heard);
                        Body::Frame(frame)
                    }
                    preview::Preview::Pdf { .. } => Body::Not(t(preview::REASONS[6]).to_string()),
                    preview::Preview::Media { video } => {
                        let ground = gpui::Rgba::from(page_ground(cx.theme()));
                        let hex = format!("{:02x}{:02x}{:02x}", (ground.r * 255.).round() as u8, (ground.g * 255.).round() as u8, (ground.b * 255.).round() as u8);
                        match crate::media::Player::open(window, &path, video, &hex) {
                            Some(player) => {
                                // Shown once the page has faded in.
                                cx.spawn(async move |this, cx| {
                                    cx.background_executor().timer(PLAYER_WAIT).await;
                                    let _ = this.update(cx, |_, cx| cx.notify());
                                })
                                .detach();
                                Body::Play(player)
                            }
                            None => Body::Not(t("Video and sound play in their own app.").to_string()),
                        }
                    }
                    preview::Preview::None(why) => Body::Not(t(&why).to_string()),
                };
                if let Some(a) = s.tabs[ix].asset.as_mut() {
                    a.body = body;
                    a._sub = sub;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The frame a tab's picture or PDF stands in, made over the tab's
    /// editor and its store (a PDF's page sizes put there first, as a
    /// note's layout puts them), and what it asks heard: the file's menu
    /// as the assets card has it, and the viewer for a picture.
    fn asset_frame(&mut self, ix: usize, pdf: Option<Vec<(f32, f32)>>, picture: Option<(u32, u32)>, cx: &mut Context<Self>) -> (Entity<Embed>, Subscription) {
        let (path, editor) = (self.tabs[ix].path.clone(), self.tabs[ix].editor.clone());
        let store = editor.read(cx).store();
        if let Some(sizes) = pdf {
            store.borrow_mut().pdfs.insert(path.clone(), crate::layout::PdfFile { sizes, ..Default::default() });
        }
        let look = self.look.clone();
        let frame = cx.new(|cx| Embed::for_tab(path, picture, editor.downgrade(), store, look, cx));
        let heard = cx.subscribe(&frame, |this, frame, ev: &TabAsk, cx| {
            let path = frame.read(cx).path().to_path_buf();
            match ev {
                TabAsk::Menu(at) => this.show_asset_menu(path, *at, true, cx),
                TabAsk::View(from) => {
                    let size = frame.read(cx).picture_size().unwrap_or((1, 1));
                    this.open_viewer(&Viewing { pictures: vec![Seen { path, size, drawn: None }], at: 0, from: *from }, cx);
                }
            }
        });
        (frame, heard)
    }

    /// The shown tab's frame, while it shows a picture or a PDF.
    fn shown_frame(&self) -> Option<Entity<Embed>> {
        let shown = self.shown.as_deref()?;
        match self.tabs.iter().find(|t| t.path == shown).and_then(|t| t.asset.as_ref()).map(|a| &a.body) {
            Some(Body::Frame(frame)) => Some(frame.clone()),
            _ => None,
        }
    }

    /// ⌘+ and ⌘- step the shown picture or PDF through its frame's zooms;
    /// ⌘0 puts it back to 100% or the room's width.
    pub(crate) fn zoom_shown(&mut self, step: Option<bool>, cx: &mut Context<Self>) {
        if let Some(frame) = self.shown_frame() {
            frame.update(cx, |m, cx| m.zoom_step(step, cx));
        }
    }

    /// What a closed asset's tab held, let go: the pages' pixels, and the
    /// player, with the keyboard back to the window.
    pub(crate) fn let_asset_go(asset: Option<Asset>, window: &mut Window, cx: &mut Context<Self>) {
        match asset.map(|a| a.body) {
            Some(Body::Frame(frame)) => frame.update(cx, |m, cx| m.let_go(window, cx)),
            Some(Body::Play(player)) => {
                drop(player);
                crate::sys::keys_to_window(window);
            }
            _ => {}
        }
    }

    /// Each tab's player shown only while its tab shows with nothing of
    /// the window's over it, and once the page has faded in: the system's
    /// view is drawn over the window's drawing.
    pub(crate) fn sync_players(&mut self, window: &mut Window) {
        let over = self.menus.open.is_some() || self.search_open || self.rename.is_some() || self.making.is_some() || self.new_vault.is_some() || self.export.is_some() || self.rail_fly.is_some();
        let shown = self.shown.clone();
        // The keyboard back to the window as a player goes from sight, not
        // at every frame one stays hidden: a call into the system a frame.
        let mut hid = false;
        for tab in &mut self.tabs {
            let Some(asset) = tab.asset.as_mut() else { continue };
            let waited = asset.opened.elapsed() >= PLAYER_WAIT;
            if let Body::Play(player) = &mut asset.body {
                let on = shown.as_deref() == Some(tab.path.as_path()) && !over && waited && player.is_placed();
                let first = player.first_sync();
                hid |= (player.show(on) || first) && !on;
            }
        }
        if hid {
            crate::sys::keys_to_window(window);
        }
    }

    /// An asset's own page, for what is not words: a picture or a PDF in
    /// its frame, a video or a sound in the system's player, or why
    /// nothing is shown.
    pub(crate) fn render_asset(&mut self, path: &Path, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(asset) = self.tabs.iter().find(|t| t.path == path).and_then(|t| t.asset.as_ref()) else { return div().into_any_element() };
        let icon = crate::file_icons::path(&name_of(path), false, false, theme.mode.is_dark());
        let body: AnyElement = match &asset.body {
            Body::Reading | Body::Words => div().into_any_element(),
            Body::Frame(frame) => div().size_full().p(px(ASSET_PAD)).child(frame.clone()).into_any_element(),
            // The system's view is put over this room as it is laid out,
            // so it moves in the frame the page does.
            Body::Play(player) => {
                let place = player.placer();
                div().size_full().p(px(ASSET_PAD)).child(canvas(move |bounds, _, _| place(bounds), |_, _, _, _| {}).size_full()).into_any_element()
            }
            Body::Not(why) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(px(14.))
                .child(img(icon).size(px(48.)))
                .child(div().text_size(px(13.)).text_color(theme.muted_foreground).child(why.clone()))
                .child(small_pill("asset-open-app", "icons/external-link.svg", t("Open with Default App"), Press::Primary, false, &theme).on_click(cx.listener({
                    let path = path.to_path_buf();
                    move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.open_outside(&path.to_string_lossy())
                    }
                })))
                .into_any_element(),
        };
        div().size_full().child(body).into_any_element()
    }

    /// An asset renamed, and the notes' links to it with it: an open
    /// note's through its editor, from what the editor holds, as one step
    /// of undo that is written with the note; every other note's file
    /// written whole (`vault::rename_asset`). What knows the asset by its
    /// path follows in `rename_entry`.
    pub(crate) fn rename_asset(&mut self, path: &Path, name: &str, cx: &mut Context<Self>) -> std::io::Result<PathBuf> {
        let root = self.vault_root().map(Path::to_path_buf).ok_or_else(|| std::io::Error::other("no vault"))?;
        let new = vault::asset_renamed(path, name)?;
        if new == path {
            return Ok(new);
        }
        let open: Vec<(usize, PathBuf)> = self.tabs.iter().enumerate().filter(|(_, t)| t.asset.is_none()).map(|(i, t)| (i, t.path.clone())).collect();
        let edits: Vec<(usize, Vec<(std::ops::Range<usize>, String)>)> = open
            .iter()
            .map(|(i, note)| {
                let text = self.tabs[*i].editor.read(cx).text();
                (*i, vault::relink(&root, note, &text, path, &new))
            })
            .filter(|(_, e)| !e.is_empty())
            .collect();
        let open_paths: Vec<PathBuf> = open.into_iter().map(|(_, p)| p).collect();
        vault::rename_asset(&root, path, &new, &open_paths)?;
        for (i, e) in edits {
            self.tabs[i].editor.update(cx, |ed, cx| ed.apply_edits(e, cx));
        }
        self.reread_assets(cx);
        Ok(new)
    }

    /// For a probe: what the shown asset's player has read.
    pub(crate) fn print_player(&self) {
        let shown = self.shown.as_deref();
        match self.tabs.iter().find(|t| Some(t.path.as_path()) == shown).and_then(|t| t.asset.as_ref()).map(|a| &a.body) {
            Some(Body::Play(player)) => player.print_state(),
            _ => eprintln!("media no player"),
        }
    }
}

/// The container a configuration names, while it is there.
pub(crate) fn container_of(cfg: &onyx_core::config::Config) -> Option<PathBuf> {
    Some(PathBuf::from(&cfg.app.container)).filter(|p| !cfg.app.container.is_empty() && p.is_dir())
}

/// The vaults to offer, for the sidebar's list, the landing page and the
/// menu bar note's: the one open first, then those in the container
/// opened before, the last opened first, then the rest of the folder's by
/// name (`vault::vaults_in`). With no container there is no vault: the
/// ones opened before in a container since closed are not offered, in
/// either window.
pub(crate) fn vault_list(folder: Option<&Path>, open: Option<&Path>) -> Vec<PathBuf> {
    let Some(folder) = folder else { return Vec::new() };
    // Asked by the page, the card, the rail and the menu bar note, each at
    // every frame: answered from memory, the disk looked at again no more
    // often than `LOOK_AGAIN`, and at once after this app's own change
    // (`forget_vaults`).
    let key = (folder.to_path_buf(), open.map(Path::to_path_buf));
    let kept = VAULTS.with_borrow(|k| k.as_ref().filter(|(at, was, _)| at.elapsed() < LOOK_AGAIN && *was == key).map(|(_, _, v)| v.clone()));
    if let Some(v) = kept {
        return v;
    }
    let listed = list_vaults(folder, open);
    VAULTS.set(Some((Instant::now(), key, listed.clone())));
    listed
}

/// How often the container's folder and the configuration are looked at
/// again on the disk while the window draws, for another process's change.
const LOOK_AGAIN: Duration = Duration::from_millis(500);

thread_local! {
    static VAULTS: std::cell::RefCell<Option<(Instant, (PathBuf, Option<PathBuf>), Vec<PathBuf>)>> = const { std::cell::RefCell::new(None) };
    static CONTAINER: std::cell::RefCell<Option<(Instant, Option<PathBuf>)>> = const { std::cell::RefCell::new(None) };
}

/// This app made, renamed or showed a vault, or chose or closed the
/// container: the lists are read again at their next asking.
pub(crate) fn forget_vaults() {
    VAULTS.set(None);
    CONTAINER.set(None);
}

/// The container chosen, for a window that holds no configuration of its
/// own (the menu bar note): the configuration read no more often than
/// `LOOK_AGAIN`.
pub(crate) fn container_now() -> Option<PathBuf> {
    if let Some(c) = CONTAINER.with_borrow(|c| c.as_ref().filter(|(at, _)| at.elapsed() < LOOK_AGAIN).map(|(_, c)| c.clone())) {
        return c;
    }
    let c = container_of(&onyx_core::config::Config::load());
    CONTAINER.set(Some((Instant::now(), c.clone())));
    c
}

fn list_vaults(folder: &Path, open: Option<&Path>) -> Vec<PathBuf> {
    let in_folder: Vec<PathBuf> = vault::vaults_in(folder);
    let mut out: Vec<PathBuf> = open.filter(|o| o.is_dir()).map(Path::to_path_buf).into_iter().collect();
    for p in crate::ui_state::UiState::load().recent_vaults {
        if p.is_dir() && in_folder.contains(&p) && !out.contains(&p) {
            out.push(p);
        }
    }
    for p in in_folder {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

pub(crate) fn vault_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string())
}

/// A small square button on a row of the tree: an icon in the muted ink,
/// lit under the pointer, its words with it.
/// A vault's mark: its first letter on a rounded tile of the accent, as a
/// space has its tile in Craft. Wherever the sidebar names a vault it
/// wears this, so a vault never reads as a folder or a note.
pub(crate) fn vault_badge(root: &Path, name: &str, size: f32, theme: &gpui_component::Theme, cx: &mut App) -> Div {
    let look = crate::folder_look::vault_look(root, cx);
    let ink = vault_ink(root, theme, cx);
    let tile = div().size(px(size)).flex_shrink_0().rounded(px(size * 0.3)).bg(ink.opacity(if theme.mode.is_dark() { 0.3 } else { 0.18 })).flex().items_center().justify_center();
    match look.icon {
        Some(glyph) => tile.child(Icon::default().path(crate::folder_look::glyph(&glyph)).with_size(px((size * 0.62).round())).text_color(ink)),
        None => {
            let letter: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            tile.text_size(px(size * 0.58)).font_weight(FontWeight::SEMIBOLD).text_color(ink).child(letter)
        }
    }
}

/// A vault's block: its row over its tree, and its row in the list of
/// vaults, each well taller than a folder's.
pub(crate) const VAULT_BLOCK_H: f32 = 48.;

/// A vault's ink: the color it was given, else none, the muted ink, as
/// the picker's empty swatch says; the accent is the app's, not a vault's.
pub(crate) fn vault_ink(root: &Path, theme: &gpui_component::Theme, cx: &mut App) -> Hsla {
    crate::folder_look::vault_look(root, cx).color.as_deref().and_then(crate::folder_look::parse_hex).unwrap_or(theme.muted_foreground)
}

/// A vault's mark where a vault stands as a block: its badge at the front
/// of a deck of two more of its shape, each a step up and to the right in
/// a fainter wash of its ink, so it reads as a store of many things and
/// never as one folder. `under` is the ground it stands on, which the
/// badge is made opaque over so the cards behind do not show through it.
pub(crate) fn vault_stack(root: &Path, name: &str, size: f32, under: Hsla, theme: &gpui_component::Theme, cx: &mut App) -> Div {
    let ink = vault_ink(root, theme, cx);
    let dim = under.l < 0.5;
    let step = (size * 0.12).round().max(2.);
    let card = |at: f32, alpha: f32| div().absolute().left(px(at)).top(px(step * 2. - at)).size(px(size)).rounded(px(size * 0.3)).bg(under.blend(ink.opacity(alpha)));
    let face = under.blend(ink.opacity(if dim { 0.34 } else { 0.2 }));
    div()
        .relative()
        .flex_shrink_0()
        .w(px(size + step * 2.))
        .h(px(size + step * 2.))
        .child(card(step * 2., if dim { 0.14 } else { 0.08 }))
        .child(card(step, if dim { 0.22 } else { 0.13 }))
        .child(div().absolute().top(px(step * 2.)).left_0().child(vault_badge(root, name, size, theme, cx).bg(face)))
}

/// A vault block's measures: the sidebar's, and the menu bar note's,
/// smaller for its narrower tree.
#[derive(Clone, Copy)]
pub(crate) struct BlockSize {
    pub h: f32,
    pub deck: f32,
    pub text: f32,
}

impl BlockSize {
    pub const SIDEBAR: Self = Self { h: VAULT_BLOCK_H, deck: 22., text: 14. };
    pub const MENU: Self = Self { h: 38., deck: 17., text: 13. };
}

/// The vault over its tree, the same in the sidebar and the menu bar
/// note, which give it its press: a block in a wash of the vault's ink
/// with an edge of it, its deck and its name. The block is the switch: no
/// arrows say so. Each has it at the tree's foot: under the card in the
/// sidebar, under the tree in the menu bar note.
/// `under` is the ground it stands on; `listed` lights it while its list
/// shows.
#[allow(clippy::too_many_arguments)]
pub(crate) fn vault_block(id: impl Into<ElementId>, root: &Path, name: &str, size: BlockSize, under: Hsla, listed: bool, theme: &gpui_component::Theme, cx: &mut App) -> Stateful<Div> {
    let ink = vault_ink(root, theme, cx);
    let dim = under.l < 0.5;
    let face = under.blend(ink.opacity(if dim { 0.16 } else { 0.1 }));
    let lit = face.blend(theme.foreground.opacity(0.06));
    let mark = vault_stack(root, name, size.deck, face, theme, cx);
    h_flex()
        .id(id.into())
        .relative()
        .flex_shrink_0()
        .h(px(size.h))
        .pl(px(size.h * 0.2))
        .pr(px(6.))
        .gap(px(size.h * 0.2))
        .items_center()
        .rounded(px(10.))
        .border_1()
        .border_color(ink.opacity(if dim { 0.3 } else { 0.24 }))
        .bg(if listed { lit } else { face })
        .text_size(px(size.text))
        .cursor_pointer()
        .hover(move |s| s.bg(lit))
        .child(mark)
        .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(name.to_string()))
}

/// The Notes tab's own tools, the same in both trees: Collapse All
/// Folders, New Note and New Folder, each a `row_tool`, for the caller to
/// give its press. New Note and New Folder stand in the columns a
/// folder's own do under the pointer.
pub(crate) fn notes_tools(prefix: &str, theme: &gpui_component::Theme) -> [Stateful<Div>; 3] {
    [
        row_tool(SharedString::from(format!("{prefix}-collapse")), "icons/copy-minus.svg", t("Collapse All Folders").into(), theme),
        row_tool(SharedString::from(format!("{prefix}-new-note")), "icons/file-plus.svg", t("New Note").into(), theme),
        row_tool(SharedString::from(format!("{prefix}-new-folder")), "icons/folder-plus.svg", t("New Folder").into(), theme),
    ]
}

/// The vault at the top of its tree, the same in both trees, after
/// Emaki's: a row of the tree's height, the vault's deck in the chevron's
/// column and its name in bold, for the caller to give the Notes tools
/// (`notes_tools`) at its right, its right padding and its presses. The
/// vault's block lists the vaults; this row is the tree's own head.
pub(crate) fn vault_root_row(id: &'static str, root: &Path, name: &str, under: Hsla, theme: &gpui_component::Theme, cx: &mut App) -> Stateful<Div> {
    let mark = vault_stack(root, name, 14., under, theme, cx);
    h_flex()
        .id(id)
        .flex_shrink_0()
        .h(px(32.))
        .pl(px(5.))
        .gap(px(7.))
        .items_center()
        .rounded(px(7.))
        .text_size(px(13.))
        .child(mark)
        .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(name.to_string()))
}

pub(crate) fn row_tool(id: impl Into<ElementId>, icon: &'static str, words: SharedString, theme: &gpui_component::Theme) -> Stateful<Div> {
    let (lit, wash, muted) = (theme.foreground, theme.foreground.opacity(0.1), theme.muted_foreground);
    // A little smaller than the icon buttons on a head, the icon theirs.
    div()
        .id(id.into())
        .size(px(22.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .text_color(muted)
        .hover(move |s| s.bg(wash).text_color(lit))
        .active(move |s| s.bg(lit.opacity(0.14)))
        .managed_tooltip(move |window, cx| crate::widgets::tip(words.clone(), window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(Icon::default().path(icon).with_size(px(14.)))
}
