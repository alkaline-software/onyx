//! Before a vault is shown, laid out as the vault itself is, so each level
//! reads the same: the sidebar's card holds what the open thing holds,
//! under a row naming it, and the button under the card names the open
//! thing and lists where else to go, with a way to close it. With a
//! container open, the card is its vaults under its row, with New Vault…,
//! and the button is the container's: the containers used before, Choose
//! Container… and Close Container. With none, the card is the containers
//! used before, and there is no button. The page has the picture and the
//! greeting of the vault's own page and what can be made next: New
//! Vault…, or with no container Choose Container….

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use super::library::{row_tool, vault_list, vault_name};
use super::{edge_fade, greeting, Shell, LIST_TOP, TREE_ROW_H};
use crate::i18n::t;
use crate::menu::Entry;
use crate::widgets::{choice_pill, kbd_hint, swallow_click, Press};

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

fn container_label(dir: &Path) -> String {
    match container_name(dir) {
        (name, parent) if parent.is_empty() => name,
        (name, parent) => format!("{name} · {parent}"),
    }
}

impl Shell {
    /// The page: the picture and the greeting and, on the vault page's
    /// pills, what can be made next: New Vault…, or with no container
    /// Choose Container….
    pub(crate) fn render_place_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let face = self.look.text_family.clone();
        // As `render_no_note` has them.
        let choice = |id: &'static str, icon: &'static str, label: &str, theme: &gpui_component::Theme| choice_pill(id, icon, label.to_string(), Press::Quiet, false, theme).min_w(px(240.)).h(px(34.)).px(px(14.)).gap(px(8.)).text_size(px(13.)).child(div().flex_1());
        let pill = match self.container() {
            Some(_) => choice("home-new-vault", "icons/plus.svg", t("New Vault…"), &theme).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.start_new_vault(window, cx)
            })),
            None => choice("home-choose-container", "icons/folder-open.svg", t("Choose Container…"), &theme).child(kbd_hint("⌘⇧O", &theme)).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.choose_container(window, cx)
            })),
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .pb(px(48.))
            .gap(px(26.))
            .child(v_flex().items_center().gap(px(12.)).child(Self::home_figure(&self.home_fig, &theme, cx.entity_id())).child(div().text_size(px(34.)).font_family(face).child(greeting(&self.user_name))))
            .child(v_flex().gap(px(8.)).child(pill))
            .into_any_element()
    }

    /// The sidebar before a vault, framed as the vault's: the card, a row
    /// naming what it lists with its buttons always at the right, the list
    /// under it; then the container's button, as the vault's stands under
    /// its tree. A press on a row goes into it; a right click gives Copy
    /// Path and Reveal in Finder.
    pub(crate) fn render_place_card(&self, ground: Hsla, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let container = self.container();
        let (lit, muted) = (theme.foreground, theme.muted_foreground);
        let wash = if theme.mode.is_dark() { lit.opacity(0.08) } else { lit.opacity(0.06) };
        // A tree's row at its top level: the chevrons' column, the mark, the name.
        let row = move |id: ElementId, icon: &'static str, name: SharedString, path: PathBuf, cx: &mut Context<Self>| {
            h_flex()
                .id(id)
                .flex_shrink_0()
                .h(px(TREE_ROW_H))
                .pl(px(8.))
                .pr(px(8.))
                .gap(px(6.))
                .items_center()
                .rounded(px(7.))
                .text_size(px(13.))
                .cursor_pointer()
                .hover(move |s| s.bg(wash))
                .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    swallow_click(window, cx);
                    this.show_recent_menu(path.clone(), ev.position, cx);
                }))
                .child(div().w(px(12.)).flex_shrink_0())
                .child(Icon::default().path(icon).with_size(px(15.)).text_color(muted))
                .child(div().flex_1().min_w_0().truncate().text_color(lit).child(name))
        };
        let (head_icon, head_name, tool, rows): (&'static str, SharedString, Stateful<Div>, Vec<AnyElement>) = match &container {
            Some(dir) => {
                let tool = row_tool("place-tool-vault", "icons/plus.svg", t("New Vault…").into(), theme).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.start_new_vault(window, cx)
                }));
                let rows = vault_list(Some(dir), None)
                    .into_iter()
                    .map(|p| {
                        let id = ElementId::Name(format!("place-vault-{}", p.display()).into());
                        row(id, "icons/book-open.svg", vault_name(&p).into(), p.clone(), cx)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.show_vault(p.clone(), window, cx)
                            }))
                            .into_any_element()
                    })
                    .collect();
                let (name, _) = container_name(dir);
                (container_icon(dir), name.into(), tool, rows)
            }
            None => {
                let tool = row_tool("place-tool-choose", "icons/folder-open.svg", t("Choose Container…").into(), theme).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.choose_container(window, cx)
                }));
                let rows = self
                    .known_containers()
                    .into_iter()
                    .map(|dir| {
                        let (name, parent) = container_name(&dir);
                        let id = ElementId::Name(format!("place-container-{}", dir.display()).into());
                        row(id, container_icon(&dir), name.into(), dir.clone(), cx)
                            .child(div().flex_shrink(1.).min_w_0().max_w(relative(0.45)).truncate().text_size(px(12.)).text_color(muted).child(parent))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.use_container(dir.clone(), window, cx)
                            }))
                            .into_any_element()
                    })
                    .collect();
                ("icons/box.svg", t("Containers").into(), tool, rows)
            }
        };
        self.scrollers.name(&self.place_scroll);
        // As the vault's own row over its tree (`render_vault_row`).
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
            .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().child(Icon::default().path(head_icon).with_size(px(15.)).text_color(muted)))
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(lit).child(head_name))
            .child(tool);
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
        v_flex()
            .flex_1()
            .min_h_0()
            .child(card)
            .when_some(container, |d, dir| d.child(self.render_container_button(&dir, ground, theme, cx)))
            .into_any_element()
    }

    /// The container's button under the card, made as the vault's
    /// (`render_vaults_button`): its mark and name and the arrows of a
    /// list. A press lists, rising from it as wide as itself, the
    /// containers used before (this one ticked), Choose Container… and
    /// Close Container; a press while it shows puts it away.
    fn render_container_button(&self, dir: &Path, ground: Hsla, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let (lit, muted, edge) = (theme.foreground, theme.muted_foreground, theme.foreground.opacity(0.1));
        let hover = ground.blend(theme.foreground.opacity(0.05));
        let listed = self.menus.open.as_ref().is_some_and(|m| m.what == "container");
        let place = self.vaults_at.clone();
        let (name, _) = container_name(dir);
        h_flex()
            .id("container-button")
            .relative()
            .flex_shrink_0()
            .mx(px(8.))
            .mb(px(10.))
            .h(px(36.))
            .px(px(10.))
            .gap(px(10.))
            .items_center()
            .rounded(px(10.))
            .border_1()
            .border_color(edge)
            .bg(if listed { hover } else { ground })
            .text_size(px(13.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                if this.menus.open.as_ref().is_some_and(|m| m.what == "container") {
                    this.menus.close(cx);
                    cx.notify();
                    return;
                }
                let b = this.vaults_at.get();
                this.show_containers_at(point(b.origin.x, b.origin.y - px(6.)), Some(b.size.width), cx);
            }))
            .child(canvas(move |b, _, _| place.set(b), |_, _, _, _| ()).absolute().top_0().left_0().size_full())
            .child(div().w(px(22.)).flex().justify_center().child(Icon::default().path(container_icon(dir)).with_size(px(15.)).text_color(theme.primary)))
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(lit).child(name))
            .child(Icon::default().path("icons/chevrons-up-down.svg").with_size(px(12.)).text_color(muted))
            .into_any_element()
    }

    fn show_containers_at(&mut self, at: Point<Pixels>, width: Option<Pixels>, cx: &mut Context<Self>) {
        let open = self.container();
        let mut entries: Vec<Entry> = self
            .known_containers()
            .into_iter()
            .map(|dir| {
                let chosen = open.as_ref() == Some(&dir);
                Entry::Choice { id: dir.to_string_lossy().to_string().into(), icon: container_icon(&dir), label: container_label(&dir).into(), picture: false, chosen }
            })
            .collect();
        entries.push(Entry::Rule);
        entries.push(Entry::choice("choose", "icons/folder-open.svg", t("Choose Container…")));
        entries.push(Entry::choice("close", "icons/close.svg", t("Close Container")));
        self.menus.show_above("container", at, width, entries);
        cx.notify();
    }

    /// The containers used before that are still there, the last first,
    /// and the one in use with them.
    pub(crate) fn known_containers(&self) -> Vec<PathBuf> {
        let ui = crate::ui_state::UiState::load();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        self.container().into_iter().chain(ui.recent_containers.into_iter().filter(|d| d.is_dir())).filter(|d| seen.insert(d.clone())).collect()
    }
}
