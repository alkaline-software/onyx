//! A menu of the app's own: at a right click, or dropped down from a
//! pill. A small card at the pointer, kept inside the window, over a
//! clear sheet any click or Escape puts away. Each choice has an icon
//! before its words, from what it does, so the words stand in one column;
//! choices of one kind stand together with a line between the groups; and
//! the card is as wide as its longest choice and no wider. `docs/design.md`
//! has the order the groups go in.
//!
//! It comes in over `MENU_IN`, rising a few pixels into place as it fades
//! in, and goes out over `MENU_OUT`, fading where it was: what was closed
//! is kept for that moment and drawn once more, taking no click. Each
//! opening has a serial in its animation's name, so it plays once. The
//! toolkit's own popover has no way out but at once, which is why this
//! is the app's.

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use crate::i18n::t;
use crate::widgets::{float_shadow, swallow_click, MENU_IN, MENU_OUT};

/// One line of a menu.
#[derive(Clone)]
pub enum Entry {
    /// A choice: what it is answered by, its icon, its words, and whether
    /// its icon is a picture in its own colors (a flag) rather than an
    /// icon in the ink.
    Choice { id: SharedString, icon: &'static str, label: SharedString, picture: bool, chosen: bool },
    /// A choice that takes something away, last and in the danger's ink.
    Danger { id: SharedString, icon: &'static str, label: SharedString },
    /// Choices of one kind in one row, as icons after the kind's name:
    /// how a link shows, a size, a place. Each icon's words come with
    /// the pointer, and the one chosen stands lit.
    Icons { label: SharedString, items: Vec<Tool> },
    /// The line between two groups.
    Rule,
}

/// One icon of a row of `Entry::Icons`.
#[derive(Clone)]
pub struct Tool {
    pub id: SharedString,
    pub icon: &'static str,
    pub label: SharedString,
    pub chosen: bool,
}

impl Tool {
    pub fn new(id: impl Into<SharedString>, icon: &'static str, label: impl Into<SharedString>, chosen: bool) -> Self {
        Tool { id: id.into(), icon, label: label.into(), chosen }
    }
}

impl Entry {
    pub fn choice(id: impl Into<SharedString>, icon: &'static str, label: impl Into<SharedString>) -> Self {
        Entry::Choice { id: id.into(), icon, label: label.into(), picture: false, chosen: false }
    }

    pub fn danger(id: impl Into<SharedString>, icon: &'static str, label: impl Into<SharedString>) -> Self {
        Entry::Danger { id: id.into(), icon, label: label.into() }
    }
}

/// A text's menu, in a note or a field: Cut and Copy with a selection,
/// and Paste with something to paste, over Select All. `Cut` and `Paste`
/// are left out by a text that takes no change.
pub fn edit_entries(selection: bool, clipboard: bool, locked: bool) -> Vec<Entry> {
    let mut items = Vec::new();
    if selection && !locked {
        items.push(Entry::choice("cut", "icons/scissors.svg", t("Cut")));
    }
    if selection {
        items.push(Entry::choice("copy", "icons/copy.svg", t("Copy")));
    }
    if clipboard && !locked {
        items.push(Entry::choice("paste", "icons/clipboard.svg", t("Paste")));
    }
    if !items.is_empty() {
        items.push(Entry::Rule);
    }
    items.push(Entry::choice("select-all", "icons/select-all.svg", t("Select All")));
    items
}

/// A choice from `edit_entries`, sent to the text it was opened on, which
/// takes the keyboard for it: what was selected has to show, and a paste
/// is typed on from.
pub fn do_edit(id: &str, focus: &FocusHandle, window: &mut Window, cx: &mut App) {
    use gpui_component::input::{Copy, Cut, Paste, SelectAll};
    window.focus(focus, cx);
    match id {
        "cut" => focus.dispatch_action(&Cut, window, cx),
        "copy" => focus.dispatch_action(&Copy, window, cx),
        "paste" => focus.dispatch_action(&Paste, window, cx),
        "select-all" => focus.dispatch_action(&SelectAll, window, cx),
        _ => {}
    }
}

#[derive(Clone)]
pub struct Menu {
    /// Which menu it is, for the view that answers its choices.
    pub what: &'static str,
    pub at: Point<Pixels>,
    pub entries: Vec<Entry>,
    pub serial: u32,
}

/// A view's menu: the one open, and the one going out.
#[derive(Default)]
pub struct Menus {
    pub open: Option<Menu>,
    pub gone: Option<(Menu, Instant)>,
    serial: u32,
}

impl Menus {
    pub fn show(&mut self, what: &'static str, at: Point<Pixels>, entries: Vec<Entry>) {
        self.serial += 1;
        self.open = Some(Menu { what, at, entries, serial: self.serial });
    }

    /// Put the open menu away, keeping it for its going.
    pub fn close<V: 'static>(&mut self, cx: &mut Context<V>) {
        if let Some(m) = self.open.take() {
            self.gone = Some((m, Instant::now()));
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(MENU_OUT + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    /// What to draw over the view: the open menu, or the one going out.
    /// `pick` answers a choice with the menu's `what` and the choice's id.
    #[allow(clippy::type_complexity)]
    pub fn render<V: 'static>(&self, pick: std::rc::Rc<dyn Fn(&mut V, &'static str, SharedString, &mut Window, &mut Context<V>)>, close: std::rc::Rc<dyn Fn(&mut V, &mut Context<V>)>, cx: &mut Context<V>) -> Option<AnyElement> {
        if let Some(m) = &self.open {
            return Some(card(m, None, pick, close, cx));
        }
        let (m, at) = self.gone.as_ref()?;
        (at.elapsed() < MENU_OUT).then(|| card(m, Some(at.elapsed()), pick, close, cx))
    }
}

#[allow(clippy::type_complexity)]
fn card<V: 'static>(m: &Menu, going: Option<Duration>, pick: std::rc::Rc<dyn Fn(&mut V, &'static str, SharedString, &mut Window, &mut Context<V>)>, close: std::rc::Rc<dyn Fn(&mut V, &mut Context<V>)>, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme().clone();
    let dark = theme.mode.is_dark();
    let what = m.what;
    let rows = m.entries.iter().map(|e| match e.clone() {
        // The dark palette's border is a shade off the card and cannot be
        // seen on it: a line of the ink, as a hover is.
        Entry::Rule => div().h(px(9.)).flex().items_center().child(div().h(px(1.)).w_full().mx(px(6.)).bg(if theme.mode.is_dark() { theme.foreground.opacity(0.14) } else { theme.border })).into_any_element(),
        Entry::Icons { label, items } => {
            let hover = if dark { theme.foreground.opacity(0.11) } else { theme.sidebar_accent };
            let lit = theme.primary.opacity(if dark { 0.24 } else { 0.14 });
            let tools = items.into_iter().map(|tool| {
                let pick = pick.clone();
                let id = tool.id.clone();
                let words = tool.label.clone();
                div()
                    .id(ElementId::Name(format!("menu-{what}-{}", tool.id).into()))
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(tool.chosen, |d| d.bg(lit))
                    .when(!tool.chosen, |d| d.hover(|s| s.bg(hover)))
                    .child(Icon::default().path(tool.icon).with_size(px(15.)).text_color(if tool.chosen { theme.primary } else { theme.muted_foreground }))
                    .managed_tooltip(move |window, cx| crate::widgets::tip(words.clone(), window, cx))
                    .when(going.is_none(), |d| {
                        d.on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            pick(this, what, id.clone(), window, cx)
                        }))
                    })
            });
            h_flex()
                .h(px(32.))
                .pl(px(10.))
                .pr(px(3.))
                .gap(px(16.))
                .items_center()
                .text_size(px(13.))
                .whitespace_nowrap()
                .text_color(theme.popover_foreground)
                .child(div().flex_1().child(label))
                .child(h_flex().gap(px(2.)).children(tools))
                .into_any_element()
        }
        entry => {
            let (id, icon, label, picture, chosen, danger) = match entry {
                Entry::Choice { id, icon, label, picture, chosen } => (id, icon, label, picture, chosen, false),
                Entry::Danger { id, icon, label } => (id, icon, label, false, false, true),
                Entry::Rule | Entry::Icons { .. } => unreachable!(),
            };
            let pick = pick.clone();
            let id2 = id.clone();
            h_flex()
                .id(ElementId::Name(format!("menu-{what}-{id}").into()))
                .h(px(30.))
                // An icon's drawing stops short of its box by about two
                // points a side, so the row is that much tighter at the
                // left: set the same both sides, the icon stood further
                // in than the words' end did.
                .pl(px(8.))
                .pr(px(10.))
                .gap(px(9.))
                .items_center()
                .rounded(px(6.))
                .text_size(px(13.))
                .whitespace_nowrap()
                .text_color(if danger { theme.danger } else { theme.popover_foreground })
                .cursor_pointer()
                // In the dark appearance the palette's hover is a shade off
                // the card and cannot be seen: a wash of the ink is.
                .hover(|s| s.bg(if dark { theme.foreground.opacity(0.11) } else { theme.sidebar_accent }))
                .map(|d| if picture { d.child(img(icon).w(px(17.)).h(px(12.)).flex_shrink_0()) } else { d.child(Icon::default().path(icon).with_size(px(15.)).text_color(if danger { theme.danger } else { theme.muted_foreground })) })
                .child(div().flex_1().child(label))
                .when(chosen, |d| d.child(Icon::default().path("icons/check.svg").with_size(px(14.)).text_color(theme.primary)))
                .when(going.is_none(), |d| {
                    d.on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        pick(this, what, id2.clone(), window, cx)
                    }))
                })
                .into_any_element()
        }
    });
    let body = v_flex()
        .p(px(5.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow(float_shadow(&theme))
        .occlude()
        .children(rows);
    let body = match going {
        None => body.with_animation(ElementId::NamedInteger("menu-in".into(), m.serial as u64), Animation::new(MENU_IN).with_easing(ease_out_quint()), |d, t| d.opacity(t).mt(px(-4. * (1. - t)))).into_any_element(),
        Some(gone) => {
            let left = 1. - (gone.as_secs_f32() / MENU_OUT.as_secs_f32()).min(1.);
            body.opacity(ease_out_quint()(left)).into_any_element()
        }
    };
    let sheet = div().absolute().inset_0();
    let sheet = match going {
        // A press anywhere off the card puts it away.
        None => sheet
            .id("menu-sheet")
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener({
                let close = close.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    close(this, cx)
                }
            }))
            .on_mouse_down(MouseButton::Right, cx.listener({
                let close = close.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    close(this, cx)
                }
            }))
            .into_any_element(),
        Some(_) => sheet.into_any_element(),
    };
    div()
        .absolute()
        .inset_0()
        .child(sheet)
        // Kept inside the window, so the room at its right is the room at its left.
        .child(anchored().position(m.at).snap_to_window_with_margin(px(8.)).child(body))
        .into_any_element()
}
