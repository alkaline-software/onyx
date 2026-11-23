//! Before a vault is shown. The container is a setting, chosen once (on
//! this page the first time, in Settings or File after), so it is not a
//! level of the sidebar: the sidebar is only ever vaults or one vault's
//! notes. With a container, the card lists its vaults as rows and the
//! page as tiles, each with the vault's mark (`library::vault_badge`) so a
//! vault never reads as a folder, and both offer New Vault…; the page
//! offers Choose Container… too. With none, the page offers Choose
//! Container… alone and the card is empty.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use super::library::{block_grounds, row_tool, vault_badge, vault_list, vault_name, VAULT_BLOCK_H};
use super::{edge_fade, greeting, Shell, LIST_TOP};
use crate::i18n::t;
use crate::widgets::{choice_pill, kbd_hint, swallow_click, Press};


/// A vault's tile on the page, and the room between tiles: three to a
/// row at most.
const TILE_W: f32 = 176.;
const TILE_H: f32 = 60.;
const TILE_GAP: f32 = 10.;

/// A vault's tile on the page before a vault, the same in the main
/// window and the menu bar note: a gray tile as New Vault…'s, its mark and
/// its name, for the caller to give its presses.
pub(crate) fn vault_tile(id: ElementId, p: &std::path::Path, theme: &gpui_component::Theme, cx: &mut App) -> Stateful<Div> {
    let name = vault_name(p);
    let badge = vault_badge(p, &name, 28., theme, cx);
    let lit = theme.foreground.opacity(if theme.mode.is_dark() { 0.09 } else { 0.06 });
    tile(id, theme)
        .hover(move |s| s.bg(lit))
        .child(badge)
        .child(div().flex_1().min_w_0().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(name))
}

/// New Vault…'s tile after the vaults', a plus in an empty tile.
pub(crate) fn new_vault_tile(id: &'static str, theme: &gpui_component::Theme) -> Stateful<Div> {
    let lit = theme.foreground.opacity(if theme.mode.is_dark() { 0.09 } else { 0.06 });
    tile(id.into(), theme)
        .hover(move |s| s.bg(lit))
        .child(div().size(px(28.)).flex_shrink_0().rounded(px(8.4)).bg(theme.foreground.opacity(0.06)).flex().items_center().justify_center().child(Icon::default().path("icons/plus.svg").with_size(px(15.)).text_color(theme.muted_foreground)))
        .child(div().flex_1().min_w_0().truncate().text_color(theme.muted_foreground).child(t("New Vault…")))
}

/// The tiles' rows, `per_row` at most to a row, centered.
pub(crate) fn tile_rows(per_row: usize) -> Div {
    h_flex().max_w(px(TILE_W * per_row as f32 + TILE_GAP * (per_row as f32 - 1.))).flex_wrap().justify_center().gap(px(TILE_GAP))
}

fn tile(id: ElementId, theme: &gpui_component::Theme) -> Stateful<Div> {
    let dark = theme.mode.is_dark();
    h_flex()
        .id(id)
        .w(px(TILE_W))
        .h(px(TILE_H))
        .px(px(12.))
        .gap(px(10.))
        .items_center()
        .rounded(px(12.))
        .border_1()
        .border_color(theme.foreground.opacity(0.1))
        .bg(theme.foreground.opacity(if dark { 0.04 } else { 0.025 }))
        .text_size(px(13.))
        .cursor_pointer()
}

impl Shell {
    /// The page: the picture and the greeting, and under them what the
    /// card has, laid out as tiles: each of the container's vaults by
    /// `vault_list`'s order, a press showing it and a right click giving
    /// Copy Path and Reveal in Finder, then New Vault…; under the tiles,
    /// Choose Container…, where another is taken or a new one made in the
    /// system's panel. With no container, Choose Container… alone. The
    /// page scrolls when the tiles outgrow it.
    pub(crate) fn render_place_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let face = self.look.text_family.clone();
        let container = self.container();
        let tiles: Vec<AnyElement> = match &container {
            None => Vec::new(),
            Some(dir) => {
                let mut tiles: Vec<AnyElement> = vault_list(Some(dir), None)
                    .into_iter()
                    .map(|p| {
                        vault_tile(ElementId::Name(format!("page-vault-{}", p.display()).into()), &p, &theme, cx)
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
                            .into_any_element()
                    })
                    .collect();
                tiles.push(
                    new_vault_tile("page-new-vault", &theme)
                        .on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.start_new_vault(window, cx)
                        }))
                        .into_any_element(),
                );
                tiles
            }
        };
        let choose = choice_pill("home-choose-container", "icons/folder-open.svg", t("Choose Container…").to_string(), Press::Quiet, false, &theme)
            .min_w(px(240.))
            .h(px(34.))
            .px(px(14.))
            .gap(px(8.))
            .text_size(px(13.))
            .child(div().flex_1())
            .child(kbd_hint("⌘⇧O", &theme))
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.choose_container(window, cx)
            }));
        // No greeting: what the page is for, as a plain instruction, ended
        // with a period (an ellipsis is a control's, one that asks more).
        // With no container, the greeting.
        let words: SharedString = match &container {
            Some(_) if tiles.len() > 1 => t("Pick a vault to start.").into(),
            Some(_) => t("Create a vault to start.").into(),
            None => greeting(&self.user_name).into(),
        };
        self.scrollers.name(&self.place_page_scroll);
        div()
            .relative()
            .size_full()
            .child(
                div().id("place-page").size_full().overflow_y_scroll().track_scroll(&self.place_page_scroll).child(
                    v_flex()
                        .min_h_full()
                        .items_center()
                        .justify_center()
                        .px(px(24.))
                        .pt(px(24.))
                        .pb(px(48.))
                        .gap(px(26.))
                        .child(v_flex().items_center().gap(px(12.)).child(Self::shelf_figure(&self.home_fig, cx.entity_id())).child(div().text_size(px(34.)).font_family(face).child(words)))
                        .when(!tiles.is_empty(), |d| d.child(tile_rows(3).children(tiles)))
                        .child(choose),
                ),
            )
            .vertical_scrollbar(&self.place_page_scroll)
            .into_any_element()
    }

    /// The sidebar's card before a vault, framed as the vault's: Vaults at
    /// its top with New Vault… always at its right, and under it the
    /// container's vaults by `vault_list`'s order, each with its mark. A
    /// press shows one; a right click gives Copy Path and Reveal in Finder.
    pub(crate) fn render_place_card(&self, ground: Hsla, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        // With no container there are no vaults to list: the card holds
        // the one thing to do, Choose Container…, at its center.
        if self.container().is_none() {
            let choose = choice_pill("side-choose-container", "icons/folder-open.svg", t("Choose Container…").to_string(), Press::Quiet, false, theme)
                .h(px(32.))
                .px(px(14.))
                .gap(px(8.))
                .text_size(px(13.))
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.choose_container(window, cx)
                }));
            return v_flex()
                .flex_1()
                .min_h_0()
                .child(v_flex().mx(px(8.)).mt(px(10.)).mb(px(8.)).flex_1().min_h_0().items_center().justify_center().rounded(px(10.)).border_1().border_color(theme.foreground.opacity(0.1)).bg(ground).child(choose))
                .into_any_element();
        }
        let (lit, muted) = (theme.foreground, theme.muted_foreground);
        let rows: Vec<AnyElement> = vault_list(self.container().as_deref(), None)
            .into_iter()
            .map(|p| {
                let name = vault_name(&p);
                let (block, hover, edge) = block_grounds(ground, theme);
                let badge = vault_badge(&p, &name, 24., theme, cx);
                h_flex()
                    .id(ElementId::Name(format!("place-vault-{}", p.display()).into()))
                    .flex_shrink_0()
                    .h(px(VAULT_BLOCK_H + 4.))
                    .pl(px(10.))
                    .pr(px(10.))
                    .gap(px(12.))
                    .items_center()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(edge)
                    .bg(block)
                    .text_size(px(14.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
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
                    .child(badge)
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(lit).child(name))
                    .into_any_element()
            })
            .collect();
        self.scrollers.name(&self.place_scroll);
        let tool = row_tool("place-tool-vault", "icons/plus.svg", t("New Vault…").into(), theme).on_click(cx.listener(|this, _, window, cx| {
            swallow_click(window, cx);
            this.start_new_vault(window, cx)
        }));
        // As a tree's head, the vaults' title and the one tool.
        let head = h_flex()
            .flex_shrink_0()
            .mx(px(8.))
            .mt(px(6.))
            .h(px(super::TREE_ROW_H))
            .pl(px(8.))
            .pr(px(4.))
            .gap(px(6.))
            .items_center()
            .text_size(px(13.))
            .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().child(Icon::default().path("icons/books.svg").with_size(px(15.)).text_color(muted)))
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(lit).child(t("Vaults")))
            .child(tool);
        let list = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(v_flex().id("place-list").size_full().px(px(8.)).pt(px(LIST_TOP)).pb(px(12.)).gap(px(6.)).overflow_y_scroll().track_scroll(&self.place_scroll).children(rows))
            .child(edge_fade(&self.place_scroll, ground, true, px(0.)))
            .child(edge_fade(&self.place_scroll, ground, false, px(9.)))
            .vertical_scrollbar(&self.place_scroll);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                v_flex()
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
                    .child(list),
            )
            .into_any_element()
    }

    /// The page's drawing before a vault: the vault page's stone with
    /// three notebooks fanned on it in place of one, each with a label of
    /// one of the icon's colors, and no pen, since nothing is being
    /// written yet; a vault is being chosen. It answers the pointer on the
    /// vault page's spring (`widgets::Fig`): the fan opens a little as the
    /// pointer comes, and the notebook under it rises out of the fan. As
    /// `home_figure`, each notebook is made as text with its turn written
    /// in, rounded to half a degree and half a point, so nothing blurs.
    /// Drawn by the page before a vault and by the menu bar note's landing
    /// page before one, each with its own spring, `view` redrawn as the
    /// pointer moves it.
    pub(crate) fn shelf_figure(home_fig: &std::rc::Rc<std::cell::RefCell<crate::widgets::Fig>>, view: EntityId) -> impl IntoElement {
        const W: f32 = 220.;
        const H: f32 = 124.;
        let (wind, near, moving) = {
            let mut fig = home_fig.borrow_mut();
            let moving = fig.step();
            let (wind, _, near) = fig.at();
            (wind, near, moving)
        };
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        let (deep, glow, teal, pages, paper, line, mark, lilac) = (c(0x171431), c(0x4A4588), c(0x6FB9C4), c(0xDAD8CF), c(0xF8F7F2), c(0x474747), c(0xE8A93A), c(0x8F88D8));
        let layer = |path: &'static str, color: Hsla| svg().path(path).absolute().left_0().top_0().w(px(W)).h(px(H)).text_color(color);
        // Left, right, then the middle in front; each turned about the
        // fan's foot, the one nearest the pointer lifted along itself.
        let books = [(-26., -1., lilac), (26., 1., teal), (0., 0., mark)];
        let mut parts: Vec<AnyElement> = Vec::new();
        for (turn, at, label) in books {
            let rise = 12. * near * (1. - (wind - at * 0.7).abs() / 0.7).max(0.);
            let turn = ((turn * (1. + 0.18 * near)) * 2.).round() / 2.;
            let rise = (rise * 2.).round() / 2.;
            // A box wide and tall enough for the fan's ends and the rise.
            let t = format!(r#"translate({:.1} 0) rotate({turn:.1} 88 126) translate(0 {:.1})"#, at * 22., -rise);
            let book = |body: String, color: Hsla| svg().data(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 180 140"><g transform="{t}">{body}</g></svg>"#).as_bytes()).absolute().left(px(27.)).top(px(-44.)).w(px(180.)).h(px(140.)).text_color(color).into_any_element();
            parts.push(book(r#"<rect x="62" y="52" width="60" height="74" rx="6" fill="currentColor"/>"#.into(), pages));
            parts.push(book(r#"<rect x="62" y="52" width="60" height="74" rx="6" fill="none" stroke="currentColor" stroke-width="1.5"/>"#.into(), line));
            parts.push(book(r#"<rect x="58" y="48" width="60" height="74" rx="6" fill="currentColor"/>"#.into(), paper));
            parts.push(book(r#"<g fill="none" stroke="currentColor" stroke-linecap="round"><rect x="58" y="48" width="60" height="74" rx="6" stroke-width="1.5"/><path d="M72 98 H104" stroke-width="3.5"/></g>"#.into(), line));
            parts.push(book(r#"<rect x="70" y="66" width="36" height="16" rx="4" fill="currentColor"/>"#.into(), label));
        }
        let fig = home_fig.clone();
        let (moved, left) = (home_fig.clone(), home_fig.clone());
        div()
            .id("shelf-fig")
            .relative()
            .w(px(W))
            .h(px(H))
            .flex_shrink_0()
            .on_mouse_move(move |e: &MouseMoveEvent, _, cx| {
                moved.borrow_mut().ask(Some(e.position));
                cx.notify(view);
            })
            .on_hover(move |over: &bool, _, cx| {
                if !*over {
                    left.borrow_mut().ask(None);
                    cx.notify(view);
                }
            })
            .child(
                canvas(move |bounds, _, _| fig.borrow_mut().bounds = bounds, move |_, _, window, _| {
                    if moving {
                        window.request_animation_frame();
                    }
                })
                .absolute()
                .inset_0(),
            )
            .child(layer("icons/home/stone-glow.svg", teal))
            .child(layer("icons/home/stone-base.svg", deep))
            .child(layer("icons/home/stone-lift.svg", glow))
            .child(layer("icons/home/stone-rim.svg", teal))
            .child(layer("icons/home/stone-edge.svg", gpui::white()))
            .children(parts)
    }

}
