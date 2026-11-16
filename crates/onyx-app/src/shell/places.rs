//! Before a vault is shown: one page with the vault page's look, and a
//! card in the sidebar where the vault's tree stands. The page has the
//! picture and the greeting of the vault's own page and, under them, the
//! container's vaults and New Vault…, or with no container New
//! Container…; Choose Container… with either. The card lists the
//! containers used before as folders, each opening to its vaults. Close
//! Vault comes back here.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use super::library::{row_tool, vault_list, vault_name};
use super::{edge_fade, greeting, Shell, LIST_TOP, TREE_ROW_H};
use crate::i18n::t;
use crate::widgets::{kbd_hint, swallow_click, FOLD_ANIM};

/// How many vaults the page offers on pills; the card lists them all.
const PAGE_VAULTS: usize = 5;
/// How far a vault's row stands in from its container's, as a tree's
/// level does.
const DEPTH: f32 = 14.;

/// A container's path as a person reads it, a step each
/// (`sys::short_path`: iCloud Drive and the home folder by name).
pub(crate) fn container_steps(dir: &Path) -> Vec<String> {
    crate::sys::short_path(dir).split(std::path::MAIN_SEPARATOR).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// A container's mark: a cloud for one in iCloud Drive, else a box.
pub(crate) fn container_icon(dir: &Path) -> &'static str {
    if crate::sys::short_path(dir).starts_with("iCloud Drive") {
        "icons/cloud.svg"
    } else {
        "icons/box.svg"
    }
}

/// A container's name and the folder it is in, so two of one name (Onyx
/// in iCloud Drive and in Documents) read apart.
fn container_name(dir: &Path) -> (String, String) {
    let mut steps = container_steps(dir);
    let name = steps.pop().unwrap_or_default();
    (name, steps.pop().unwrap_or_default())
}

impl Shell {
    /// The page: the picture and the greeting, as the vault's page has
    /// them, and under them on pills of one width, a long name cut short,
    /// the container's vaults (the last shown first) and New Vault…, or
    /// with no container New Container…; Choose Container… with either.
    pub(crate) fn render_place_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let face = self.look.text_family.clone();
        // The vault page's pills (`render_no_note`).
        let pill = |id: ElementId, icon: &'static str, label: SharedString, theme: &gpui_component::Theme| {
            let (ink, edge, hover) = (theme.foreground, theme.foreground.opacity(0.16), theme.foreground.opacity(0.06));
            h_flex()
                .id(id)
                .w(px(260.))
                .h(px(34.))
                .px(px(14.))
                .gap(px(8.))
                .flex_shrink_0()
                .items_center()
                .rounded_full()
                .text_size(px(13.))
                .text_color(ink)
                .border_1()
                .border_color(edge)
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(Icon::default().path(icon).with_size(px(14.)).text_color(ink))
                .child(div().flex_1().min_w_0().truncate().child(label))
        };
        let head = v_flex().items_center().gap(px(12.)).child(Self::home_figure(&self.home_fig, &theme, cx.entity_id())).child(div().text_size(px(34.)).font_family(face).child(greeting(&self.user_name)));
        let mut choices = v_flex().gap(px(8.));
        match self.container() {
            Some(dir) => {
                let vaults = vault_list(Some(&dir), None);
                let listed = !vaults.is_empty();
                choices = choices.children(vaults.into_iter().take(PAGE_VAULTS).enumerate().map(|(i, p)| {
                    pill(("place-vault", i).into(), "icons/book-open.svg", vault_name(&p).into(), &theme)
                        .on_mouse_down(MouseButton::Right, cx.listener({
                            let p = p.clone();
                            move |this, ev: &MouseDownEvent, window, cx| {
                                swallow_click(window, cx);
                                this.show_recent_menu(p.clone(), ev.position, cx);
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.show_vault(p.clone(), window, cx)
                        }))
                }));
                choices = choices.child(pill("place-new-vault".into(), "icons/plus.svg", t("New Vault…").into(), &theme).when(listed, |d| d.mt(px(8.))).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.start_new_vault(window, cx)
                })));
            }
            None => {
                choices = choices.child(pill("place-new-container".into(), "icons/plus.svg", t("New Container…").into(), &theme).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.start_new_container(window, cx)
                })));
            }
        }
        choices = choices.child(pill("place-choose-container".into(), "icons/folder-open.svg", t("Choose Container…").into(), &theme).child(kbd_hint("⌘⇧O", &theme)).on_click(cx.listener(|this, _, window, cx| {
            swallow_click(window, cx);
            this.choose_container(window, cx)
        })));
        v_flex().size_full().items_center().justify_center().pb(px(48.)).gap(px(26.)).child(head).child(choices).into_any_element()
    }

    /// Whether a container's vaults show under it: the one in use opens
    /// to begin with and the rest are shut, each turned by a press.
    fn place_open(&self, dir: &Path) -> bool {
        (self.container().as_deref() == Some(dir)) != self.place_turned.contains(dir)
    }

    /// The sidebar's card, framed as the vault's: Containers at its top
    /// with New Container… and Choose Container… always at its right, and
    /// under it the containers used before as a tree's folders, each by
    /// its own name with the folder it is in muted at the right. A press
    /// on a container opens or shuts it, its vaults under it a level in;
    /// under the pointer it has New Vault…, made in it, as a folder has
    /// its own buttons. A press on a vault shows it, its container taken.
    pub(crate) fn render_place_card(&self, ground: Hsla, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let (lit, muted) = (theme.foreground, theme.muted_foreground);
        let wash = if theme.mode.is_dark() { lit.opacity(0.08) } else { lit.opacity(0.06) };
        let row = move |id: ElementId, depth: usize| {
            h_flex()
                .id(id)
                .flex_shrink_0()
                .h(px(TREE_ROW_H))
                .pl(px(8. + DEPTH * depth as f32))
                .pr(px(4.))
                .gap(px(6.))
                .items_center()
                .rounded(px(7.))
                .text_size(px(13.))
                .cursor_pointer()
                .hover(move |s| s.bg(wash))
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        for dir in self.known_containers() {
            let open = self.place_open(&dir);
            let (name, parent) = container_name(&dir);
            let hovered = self.hover_row.as_ref() == Some(&dir);
            rows.push(
                row(ElementId::Name(format!("place-container-{}", dir.display()).into()), 0)
                    .on_hover(cx.listener({
                        let dir = dir.clone();
                        move |this, on: &bool, _, cx| {
                            if *on {
                                this.hover_row = Some(dir.clone());
                            } else if this.hover_row.as_ref() == Some(&dir) {
                                this.hover_row = None;
                            }
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(MouseButton::Right, cx.listener({
                        let dir = dir.clone();
                        move |this, ev: &MouseDownEvent, window, cx| {
                            swallow_click(window, cx);
                            this.show_recent_menu(dir.clone(), ev.position, cx);
                        }
                    }))
                    .on_click(cx.listener({
                        let dir = dir.clone();
                        move |this, _, window, cx| {
                            swallow_click(window, cx);
                            if !this.place_turned.remove(&dir) {
                                this.place_turned.insert(dir.clone());
                            }
                            cx.notify();
                        }
                    }))
                    .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().child(Icon::default().path(if open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" }).with_size(px(12.)).text_color(muted)))
                    .child(Icon::default().path(container_icon(&dir)).with_size(px(15.)).text_color(muted))
                    .child(div().flex_1().min_w_0().truncate().text_color(lit).child(name))
                    .when(!hovered, |d| d.child(div().flex_shrink(1.).min_w_0().max_w(relative(0.45)).truncate().pr(px(4.)).text_size(px(12.)).text_color(muted).child(parent)))
                    .when(hovered, |d| {
                        let dir = dir.clone();
                        d.child(row_tool(ElementId::Name(format!("place-new-vault-{}", dir.display()).into()), "icons/plus.svg", t("New Vault…").into(), theme).on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.use_container(dir.clone(), window, cx);
                            this.start_new_vault(window, cx);
                        })))
                    })
                    .into_any_element(),
            );
            // Its vaults, in a box that runs up to their height and back
            // as a tree's folder does (`Motion::presence`).
            let key = format!("place-fold-{}", dir.display());
            let moving = self.motion.presence(&key, open, cx);
            if !open && moving.is_none() {
                continue;
            }
            let vaults: Vec<AnyElement> = vault_list(Some(&dir), None)
                .into_iter()
                .map(|p| {
                    let container = dir.clone();
                    let name = vault_name(&p);
                    row(ElementId::Name(format!("place-vault-row-{}", p.display()).into()), 1)
                        .on_mouse_down(MouseButton::Right, cx.listener({
                            let p = p.clone();
                            move |this, ev: &MouseDownEvent, window, cx| {
                                swallow_click(window, cx);
                                this.show_recent_menu(p.clone(), ev.position, cx);
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.use_container(container.clone(), window, cx);
                            this.show_vault(p.clone(), window, cx)
                        }))
                        .child(div().w(px(12.)).flex_shrink_0())
                        .child(Icon::default().path("icons/book-open.svg").with_size(px(15.)).text_color(muted))
                        .child(div().flex_1().min_w_0().truncate().text_color(lit).child(name))
                        .into_any_element()
                })
                .collect();
            let full = vaults.len() as f32 * TREE_ROW_H;
            let group = v_flex().flex_shrink_0().overflow_hidden().child(v_flex().flex_shrink_0().children(vaults));
            rows.push(match moving {
                Some((opening, serial)) => group
                    .with_animation(ElementId::NamedInteger(key.into(), serial as u64), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                        let t = if opening { t } else { 1. - t };
                        d.h(px(full * t)).opacity(t)
                    })
                    .into_any_element(),
                None => group.into_any_element(),
            });
        }
        self.scrollers.name(&self.place_scroll);
        let tools = h_flex()
            .flex_shrink_0()
            .gap(px(2.))
            .child(row_tool("place-tool-new", "icons/plus.svg", t("New Container…").into(), theme).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.start_new_container(window, cx)
            })))
            .child(row_tool("place-tool-choose", "icons/folder-open.svg", t("Choose Container…").into(), theme).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.choose_container(window, cx)
            })));
        let head = h_flex()
            .flex_shrink_0()
            .mx(px(8.))
            .mt(px(6.))
            .h(px(TREE_ROW_H))
            .pl(px(8.))
            .pr(px(4.))
            .gap(px(6.))
            .items_center()
            .text_size(px(13.))
            .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().child(Icon::default().path("icons/box.svg").with_size(px(15.)).text_color(muted)))
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(lit).child(t("Containers")))
            .child(tools);
        let list = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(v_flex().id("place-list").size_full().px(px(8.)).pt(px(LIST_TOP)).pb(px(12.)).overflow_y_scroll().track_scroll(&self.place_scroll).children(rows))
            .child(edge_fade(&self.place_scroll, ground, true, px(0.)))
            .child(edge_fade(&self.place_scroll, ground, false, px(9.)))
            .vertical_scrollbar(&self.place_scroll);
        let card = v_flex()
            .mx(px(8.))
            .mt(px(10.))
            .mb(px(8.))
            .flex_1()
            .min_h_0()
            .rounded(px(10.))
            .border_1()
            .border_color(lit.opacity(0.1))
            .bg(ground)
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(head)
            .child(list);
        v_flex().flex_1().min_h_0().child(card).into_any_element()
    }

    /// The containers used before that are still there, the last first,
    /// and the one in use with them.
    pub(crate) fn known_containers(&self) -> Vec<PathBuf> {
        let ui = crate::ui_state::UiState::load();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        self.container().into_iter().chain(ui.recent_containers.into_iter().filter(|d| d.is_dir())).filter(|d| seen.insert(d.clone())).collect()
    }
}
