//! A link on a line of its own shown as a card, as Craft's link and file
//! blocks have them, in three sizes (`html::CardSize`): Small, one row of
//! its icon, its name and where it is; Medium, its name, a few words of it
//! and where it is, its picture beside them; Large, its picture over them.
//! The line holds the link alone, `<a href data-card>words</a>`, so any app
//! reads it as the link it is (`editor/cards.rs` changes it).
//!
//! A card of a file of the vault shows the file's own icon, its name, its
//! kind and size, and for a PDF its first page, for a picture the picture,
//! for a document the start of its words. A card of a page of the web
//! shows what the page's head says of it (`onyx_core::link_card`), read
//! once over the network and kept in the cache; until it is read, and
//! with no network, the link's words and its site.
//!
//! A card is one piece of the note, as a picture is: a press selects it,
//! a double press (or one as the note reads) opens what it leads to, a
//! right click gives its menu. Its height goes by its size alone, so
//! nothing moves as what it shows arrives. `docs/editor.md`, Cards.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use onyx_core::html::CardSize;
use onyx_core::link_card::{self, LinkCard};

use crate::editor::{Editor, EditorEvent};
use crate::embed::CONTROL_ROUND;

/// A card's heights, and a large one's widest and its words' part.
const SMALL_H: f32 = 52.;
const MEDIUM_H: f32 = 116.;
const LARGE_W: f32 = 520.;
const LARGE_WORDS: f32 = 100.;
/// The picture beside a medium card's words: as wide as this many times
/// its height.
const MEDIUM_PICTURE: f32 = 1.5;
/// How many characters of a document a card shows.
const EXCERPT: usize = 600;

/// How wide and tall a card of a size stands in this much room.
pub(crate) fn card_size(size: CardSize, room: f32) -> (f32, f32) {
    match size {
        CardSize::Small => (room, SMALL_H),
        CardSize::Medium => (room, MEDIUM_H),
        CardSize::Large => {
            let w = room.min(LARGE_W).round();
            (w, (w * 0.5).round() + LARGE_WORDS)
        }
    }
}

/// What a card knows of a file of the vault.
#[derive(Clone, Default)]
struct FileFacts {
    name: String,
    /// Its kind and size: `PDF · 2.4 MB`.
    meta: String,
    excerpt: Option<String>,
}

pub(crate) struct Card {
    /// Where it leads, as the line writes it, and the file it names or,
    /// for the web, its address.
    href: String,
    path: PathBuf,
    web: bool,
    /// Which of the note's cards of this link it is, the first being 0.
    nth: usize,
    note: WeakEntity<Editor>,
    /// Its size, its words and how large, which the note's layout gives it.
    pub(crate) size: (f32, f32),
    pub(crate) layout: CardSize,
    pub(crate) words: String,
    link: Option<LinkCard>,
    file: Option<FileFacts>,
    icon: Option<Arc<RenderImage>>,
    cover: Option<Arc<RenderImage>>,
    pub(crate) hovered: bool,
}

impl Card {
    pub(crate) fn new(href: String, path: PathBuf, web: bool, nth: usize, note: WeakEntity<Editor>, cx: &mut Context<Self>) -> Self {
        let mut me = Self { href, path, web, nth, note, size: (0., 0.), layout: CardSize::Small, words: String::new(), link: None, file: None, icon: None, cover: None, hovered: false };
        if me.web {
            me.read_page(cx);
        } else {
            me.read_file(cx);
        }
        me
    }

    /// The page's card as the cache keeps it, then read again over the
    /// network when it was never read or long ago.
    fn read_page(&mut self, cx: &mut Context<Self>) {
        let href = self.href.clone();
        cx.spawn(async move |this, cx| {
            let kept = cx.background_spawn({
                let href = href.clone();
                async move { link_card::cached(&href).map(|k| (k.clone(), pictures_of(&k))) }
            })
            .await;
            let stale = kept.as_ref().is_none_or(|(k, _)| k.stale());
            if let Some((k, (icon, cover))) = kept {
                let _ = this.update(cx, |me, cx| me.show_page(k, icon, cover, cx));
            }
            if stale {
                let fresh = cx.background_spawn(async move { link_card::fetch(&href).ok().map(|k| (k.clone(), pictures_of(&k))) }).await;
                if let Some((k, (icon, cover))) = fresh {
                    let _ = this.update(cx, |me, cx| me.show_page(k, icon, cover, cx));
                }
            }
        })
        .detach();
    }

    fn show_page(&mut self, link: LinkCard, icon: Option<Arc<RenderImage>>, cover: Option<Arc<RenderImage>>, cx: &mut Context<Self>) {
        self.link = Some(link);
        self.icon = icon;
        self.cover = cover;
        cx.notify();
    }

    /// The file's facts and its picture, read off the window's thread.
    fn read_file(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        cx.spawn(async move |this, cx| {
            let (facts, cover) = cx.background_spawn(async move { file_facts(&path) }).await;
            let _ = this.update(cx, |me, cx| {
                me.file = Some(facts);
                me.cover = cover;
                cx.notify();
            });
        })
        .detach();
    }

    /// The line it stands on.
    fn line(&self, cx: &App) -> Option<usize> {
        let key = (self.href.clone(), self.nth);
        self.note.upgrade().and_then(|n| n.read(cx).line_of_card(&key))
    }

    /// What it leads to opened: the page in the browser, the file as a
    /// link to it opens.
    fn open(&mut self, cx: &mut Context<Self>) {
        let Some(note) = self.note.upgrade() else { return };
        let href = self.href.clone();
        cx.defer(move |cx| note.update(cx, |_, cx| cx.emit(EditorEvent::OpenLink(href))));
    }

    fn press(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let (Some(line), Some(note)) = (self.line(cx), self.note.upgrade()) else { return };
        if ev.click_count >= 2 || note.read(cx).reads() {
            self.open(cx);
            return;
        }
        note.update(cx, |e, cx| e.select_object(line, window, cx));
    }

    fn right_press(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let (Some(line), Some(note)) = (self.line(cx), self.note.upgrade()) else { return };
        let file = (!self.web).then(|| self.path.clone());
        let menu = note.update(cx, |e, cx| {
            e.select_object(line, window, cx);
            e.piece_menu(line, file)
        });
        let at = ev.position;
        cx.defer(move |cx| note.update(cx, |_, cx| cx.emit(EditorEvent::Menu(at, menu))));
    }

    /// Its name: the page's title, else the link's words, else its site;
    /// a file's, the link's words, else the file's name.
    fn title(&self) -> String {
        let words = self.words.trim();
        if self.web {
            self.link.as_ref().and_then(|l| l.title.clone()).filter(|t| !t.is_empty()).or_else(|| (!words.is_empty() && words != self.href).then(|| words.to_string())).unwrap_or_else(|| link_card::host(&self.href))
        } else if !words.is_empty() {
            words.to_string()
        } else {
            self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        }
    }

    /// Where it is: the site, or the file's kind and size.
    fn meta(&self) -> String {
        if self.web {
            self.link.as_ref().and_then(|l| l.site.clone()).unwrap_or_else(|| link_card::host(&self.href))
        } else {
            self.file.as_ref().map(|f| f.meta.clone()).unwrap_or_default()
        }
    }

    /// A few words of it under its name: the page's description, or a
    /// document's start, which a large card shows as its picture instead.
    fn words_of(&self) -> Option<String> {
        if self.web {
            self.link.as_ref().and_then(|l| l.description.clone())
        } else if self.layout == CardSize::Large {
            None
        } else {
            self.file.as_ref().and_then(|f| f.excerpt.clone())
        }
    }

    /// Its icon: the page's, else a globe; the file's kind's.
    fn icon_el(&self, size: f32, cx: &App) -> AnyElement {
        if self.web {
            match &self.icon {
                Some(icon) => img(icon.clone()).size(px(size)).rounded(px(size * 0.22)).flex_shrink_0().into_any_element(),
                None => Icon::default().path("icons/globe.svg").with_size(px(size)).text_color(cx.theme().muted_foreground).flex_shrink_0().into_any_element(),
            }
        } else {
            let name = self.file.as_ref().map(|f| f.name.clone()).unwrap_or_else(|| self.path.to_string_lossy().to_string());
            img(crate::file_icons::path(&name, false, false, cx.theme().mode.is_dark())).size(px(size)).flex_shrink_0().into_any_element()
        }
    }

    /// Where the card's site or kind stands under its words: its icon,
    /// then what it says.
    fn foot(&self, cx: &App) -> Div {
        let theme = cx.theme();
        h_flex().flex_shrink_0().h(px(18.)).gap(px(6.)).items_center().min_w_0().child(self.icon_el(14., cx)).child(div().min_w_0().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(self.meta()))
    }

    /// The card's picture filling `w` by `h`, or with none, what stands in
    /// its place: a document's words, else its icon large on a quiet ground.
    fn picture(&self, round: Corners<Pixels>, cx: &App) -> Div {
        let theme = cx.theme();
        let ground = corners(div().size_full().overflow_hidden(), round).bg(theme.muted.opacity(0.6));
        let page = self.file.as_ref().and_then(|f| f.excerpt.clone()).filter(|_| self.layout == CardSize::Large);
        match (&self.cover, page) {
            (Some(cover), _) => ground.child(corners(img(cover.clone()).size_full().object_fit(ObjectFit::Cover), round)),
            (None, Some(words)) => ground.px(px(16.)).py(px(12.)).child(div().text_size(px(11.)).line_height(px(16.)).text_color(theme.muted_foreground).child(words)),
            (None, None) => ground.flex().items_center().justify_center().child(self.icon_el(32., cx)),
        }
    }
}

/// Each corner as round as it says.
fn corners<E: Styled>(e: E, round: Corners<Pixels>) -> E {
    e.rounded_tl(round.top_left).rounded_tr(round.top_right).rounded_bl(round.bottom_left).rounded_br(round.bottom_right)
}

/// A page's icon and picture, from the cache's files.
fn pictures_of(k: &LinkCard) -> (Option<Arc<RenderImage>>, Option<Arc<RenderImage>>) {
    (k.icon.as_deref().and_then(crate::layout::read_picture), k.image.as_deref().and_then(crate::layout::read_picture))
}

/// What a card shows of a file: its name, kind and size, the start of
/// its words, and its picture: a PDF's first page, a picture itself.
fn file_facts(path: &Path) -> (FileFacts, Option<Arc<RenderImage>>) {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let kind = path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
    let bytes = std::fs::metadata(path).map_or(0, |m| m.len());
    let meta = [kind, size_words(bytes)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    let mut excerpt = None;
    let cover = if onyx_core::vault::is_pdf(path) {
        onyx_core::preview::pdf_page(path, 0, 720).ok().and_then(|(w, h, rgba)| crate::layout::bgra(w, h, rgba))
    } else if onyx_core::vault::is_picture(path) {
        crate::layout::read_picture(path)
    } else {
        if let onyx_core::preview::Preview::Markdown { text, .. } = onyx_core::preview::read(path) {
            excerpt = Some(plain_start(&text, EXCERPT)).filter(|s| !s.is_empty());
        }
        None
    };
    (FileFacts { name, meta, excerpt }, cover)
}

/// A file's size as a person reads it.
fn size_words(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0 => String::new(),
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.0} KB", b / 1024.),
        n if n < 1024 * 1024 * 1024 => format!("{:.1} MB", b / 1024. / 1024.),
        _ => format!("{:.1} GB", b / 1024. / 1024. / 1024.),
    }
}

/// The start of a document's words, with its marks at the lines' heads
/// and its front matter left out, up to `most` characters.
fn plain_start(text: &str, most: usize) -> String {
    let mut out = String::new();
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim() == "---" {
                break;
            }
        }
    }
    for line in lines {
        let l = line.trim().trim_start_matches(['#', '>', '-', '*', '+', '|', ' ']).trim();
        if l.is_empty() || l.starts_with("```") || l.starts_with("~~~") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(l);
        if out.chars().count() >= most {
            break;
        }
    }
    out.chars().take(most).collect()
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (w, h) = self.size;
        let round = px(CONTROL_ROUND - 1.);
        let title = |size: f32, clamp: usize| div().flex_shrink_0().min_w_0().text_size(px(size)).line_height(px(size + 5.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).line_clamp(clamp).text_ellipsis().child(self.title());
        let words = self.words_of().map(|d| div().flex_shrink_0().min_w_0().text_size(px(13.)).line_height(px(18.)).text_color(theme.muted_foreground).line_clamp(2).text_ellipsis().child(d));
        let body = match self.layout {
            CardSize::Small => h_flex()
                .size_full()
                .px(px(14.))
                .gap(px(10.))
                .items_center()
                .child(self.icon_el(20., cx))
                .child(div().flex_1().min_w_0().truncate().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(self.title()))
                .child(div().flex_shrink_0().max_w(px(w * 0.4)).truncate().text_size(px(12.5)).text_color(theme.muted_foreground).child(self.meta()))
                .into_any_element(),
            CardSize::Medium => {
                let shows = self.cover.is_some() || !self.web;
                let side = (h * MEDIUM_PICTURE).round().min(w * 0.4);
                h_flex()
                    .size_full()
                    .child(v_flex().flex_1().min_w_0().h_full().px(px(16.)).py(px(14.)).gap(px(4.)).child(title(15., 1)).children(words).child(div().flex_1()).child(self.foot(cx)))
                    .when(shows, |d| d.child(div().w(px(side)).h_full().flex_shrink_0().border_l_1().border_color(theme.border).child(self.picture(Corners { top_left: px(0.), bottom_left: px(0.), top_right: round, bottom_right: round }, cx))))
                    .into_any_element()
            }
            CardSize::Large => {
                let cover_h = (h - LARGE_WORDS).max(0.);
                v_flex()
                    .size_full()
                    .child(div().w_full().h(px(cover_h)).flex_shrink_0().border_b_1().border_color(theme.border).child(self.picture(Corners { top_left: round, top_right: round, bottom_left: px(0.), bottom_right: px(0.) }, cx)))
                    .child(v_flex().flex_1().min_w_0().px(px(16.)).py(px(12.)).gap(px(4.)).child(title(16., 1)).children(words).child(div().flex_1()).child(self.foot(cx)))
                    .into_any_element()
            }
        };
        div()
            .id("card")
            .occlude()
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(CONTROL_ROUND))
            .border_1()
            .border_color(if self.hovered { theme.muted_foreground.opacity(0.4) } else { theme.border })
            .bg(theme.background)
            .on_hover(cx.listener(|me, on: &bool, _, cx| {
                me.hovered = *on;
                cx.notify();
                // The note lays out a large card's bar, which comes with it.
                if let Some(note) = me.note.upgrade() {
                    note.update(cx, |_, cx| cx.notify());
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::press))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::right_press))
            .child(body)
            // A picture made to cover its room is drawn past it, so its own
            // corners are not the card's: what lies past them is covered.
            .child(crate::embed::corners(theme.background, CONTROL_ROUND - 1.))
    }
}

#[cfg(test)]
mod tests {
    use super::{card_size, plain_start, size_words, CardSize, LARGE_W, LARGE_WORDS, MEDIUM_H, SMALL_H};

    #[test]
    fn a_card_is_as_tall_as_its_size_says() {
        assert_eq!(card_size(CardSize::Small, 700.), (700., SMALL_H));
        assert_eq!(card_size(CardSize::Medium, 300.), (300., MEDIUM_H));
        assert_eq!(card_size(CardSize::Large, 800.), (LARGE_W, LARGE_W / 2. + LARGE_WORDS));
        assert_eq!(plain_start("---\ntitle: x\n---\n# Head\n\n- one\n- two", 100), "Head one two");
        assert_eq!(size_words(2_500_000), "2.4 MB");
    }
}
