//! The settings window (⌘,): a window of its own, as the Mac's own
//! settings are. The rail of sections at its left under the traffic
//! lights; at its right the chosen section's name in the window's top,
//! level with the traffic lights, over its groups, each a card of rows
//! with a name at the left and a control at the right.
//!
//! The window draws the main view's own state (`SettingsView` renders
//! `Shell::render_settings`), so every listener on it is the main view's
//! and a change shows in both windows at once. Every section fits the
//! window as it opens; the page scrolls only for a window made smaller.

use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use onyx_core::config::Config;
use onyx_core::update;

use crate::i18n::{phrase, t, tf, weave};
use crate::menu::Entry;
use crate::shell::{Escape, OpenSettings, Shell};
use crate::widgets::{choice_pill, page_in, small_pill, swallow_click, tip, wordmark, Press};

/// A row of the list of the system's faces.
const FONT_ROW_H: f32 = 30.;
/// A CJK tile's sample: a face's name in its own writing, four characters
/// that fit the narrowest tile.
/// A monospace tile's sample, ten characters in the same room.
const MONO_SAMPLE_PX: f32 = 12.;
/// A face's tile, as tall as Emaki's: the three rows fit the page.
const TILE_H: f32 = 54.;
/// The room round a row of tiles and between two tiles.
const TILE_PAD: f32 = 10.;
const TILE_GAP: f32 = 6.;
const CJK_SAMPLE_PX: f32 = 16.;

/// The window's size as it opens, and the least it is dragged to. It is
/// never narrower than it opens: the rows are laid out for that width,
/// and narrower, names broke mid-word. As tall as General, the longest
/// page, needs; shorter, the page scrolls.
pub const SETTINGS_W: Pixels = px(820.);
pub const SETTINGS_H: Pixels = px(640.);
const SETTINGS_MIN: Size<Pixels> = Size { width: SETTINGS_W, height: px(460.) };
const RAIL_W: Pixels = px(190.);
const PAGE_X: Pixels = px(36.);
/// The window's top, clear for the traffic lights and dragged by.
const TOP_H: Pixels = px(52.);
/// What the widest controls at a row's right take, so those on a page
/// start and end together.
const CONTROL_W: Pixels = px(336.);
pub(crate) const CONTROL_H: Pixels = px(28.);
const CARD_R: Pixels = px(12.);
pub const SETTINGS_CONTEXT: &str = "Settings";

/// The sections in the rail's order: key, name, icon. About is last.
pub const SECTIONS: &[(&str, &str, &str)] = &[
    ("general", phrase("General"), "icons/settings.svg"),
    ("font", phrase("Font"), "icons/a-large-small.svg"),
    ("terminal", phrase("Terminal"), "icons/terminal.svg"),
    ("menubar", phrase("Menu Bar"), "icons/menu-bar.svg"),
    ("about", phrase("About"), "icons/info.svg"),
];

/// A section by its key, the first when there is none of that name.
pub fn section_named(key: &str) -> &'static str {
    // The keys the window remembers from before General and Font.
    let key = match key {
        "appearance" | "size" | "spelling" => "general",
        "text" | "fonts" => "font",
        k => k,
    };
    SECTIONS.iter().find(|(k, _, _)| *k == key).map_or(SECTIONS[0].0, |(k, _, _)| k)
}

/// Whether a menu of the main view's is the settings window's to draw.
/// The main window draws every other: both draw the main view's menus,
/// each in its own coordinates, so one drawn by both stood in two places.
pub fn menu_is_ours(what: &str) -> bool {
    matches!(what, "language" | "writing-language") || what == crate::shell::SETTINGS_EDIT
}

/// The settings window's root: the main view's settings, drawn in a
/// window of their own.
pub struct SettingsView {
    shell: WeakEntity<Shell>,
    focus: FocusHandle,
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone();
        match self.shell.upgrade() {
            Some(s) => s.update(cx, |s, cx| s.render_settings(focus, window, cx)),
            None => div().into_any_element(),
        }
    }
}

/// Show the settings: the window comes forward, or opens, centered as a
/// Mac's settings window opens.
pub fn open(shell: WeakEntity<Shell>, cx: &mut App) {
    let Some(s) = shell.upgrade() else { return };
    // Login Items is the system's, and may have changed since.
    s.update(cx, |s, cx| s.read_at_login(cx));
    if let Some(handle) = s.read(cx).settings {
        if handle.update(cx, |_, window, _| window.activate_window()).is_ok() {
            return;
        }
    }
    let bounds = Bounds::centered(None, size(SETTINGS_W, SETTINGS_H), cx);
    let titlebar = if cfg!(target_os = "macos") {
        TitlebarOptions { title: Some(t("Settings").into()), appears_transparent: true, traffic_light_position: Some(point(px(17.), px(19.))) }
    } else {
        TitlebarOptions { title: Some(t("Settings").into()), appears_transparent: false, traffic_light_position: None }
    };
    let weak = shell.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(titlebar),
            window_min_size: Some(SETTINGS_MIN),
            app_id: Some("Onyx".into()),
            // The top is the app's to drag by: left to AppKit, a press on
            // a control there moved the window.
            app_owns_titlebar_drag: true,
            ..Default::default()
        },
        move |window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            // The installed-font fields are made with this window, since
            // an input watches the window it was made in.
            if let Some(s) = weak.upgrade() {
                s.update(cx, |s, cx| {
                    s.make_font_query(window, cx);
                    s.make_name_input(window, cx);
                });
            }
            let view = cx.new(|cx| {
                if let Some(s) = weak.upgrade() {
                    cx.observe(&s, |_, _, cx| cx.notify()).detach();
                }
                SettingsView { shell: weak.clone(), focus }
            });
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        },
    );
    match opened {
        Ok(handle) => {
            s.update(cx, |s, _| s.settings = Some(handle.into()));
            let _ = handle.update(cx, |_, window, _| {
                if std::env::var_os("ONYX_GO").is_some() {
                    crate::sys::keep_on_top(window);
                }
                window.activate_window();
            });
        }
        Err(e) => eprintln!("onyx: could not open the settings: {e}"),
    }
}

/// A group of the settings: its name over it, what belongs to the whole
/// group at the name's right, and its rows on one card.
/// The languages a note can be checked in: the key `check::LANGUAGES`
/// has, the flag, and the name.
const WRITING: &[(&str, &str, &str)] = &[("en-US", "icons/flags/us.svg", phrase("English (US)")), ("en-GB", "icons/flags/gb.svg", phrase("English (UK)")), ("es", "icons/flags/es.svg", phrase("Spanish")), ("fr", "icons/flags/fr.svg", phrase("French")), ("de", "icons/flags/de.svg", phrase("German"))];

pub(crate) fn group(title: Option<SharedString>, aside: Option<AnyElement>, rows: Vec<AnyElement>, theme: &gpui_component::Theme) -> AnyElement {
    let head = (title.is_some() || aside.is_some()).then(|| {
        h_flex()
            .min_h(px(20.))
            // In line with the rows' names and controls under it.
            .px(px(14.))
            .gap(px(12.))
            .items_center()
            .justify_between()
            .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).children(title))
            .children(aside)
    });
    v_flex().gap(px(6.)).children(head).child(card(rows, theme)).into_any_element()
}

/// The ground of a group: a card a shade off the page, with a hairline
/// between its rows. A row that paints its own ground rounds it where it
/// meets the card's corners, by the card's radius less its border: a clip
/// in gpui is a rectangle.
fn card(rows: Vec<AnyElement>, theme: &gpui_component::Theme) -> Div {
    v_flex().w_full().rounded(CARD_R).border_1().border_color(theme.border).bg(card_ground(theme)).children(rows.into_iter().enumerate().map(|(i, row)| div().when(i > 0, |d| d.border_t_1().border_color(theme.border)).child(row)))
}

/// A card's ground: the muted shade thinned in the light appearance, the
/// sidebar's in the dark.
fn card_ground(theme: &gpui_component::Theme) -> Hsla {
    if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.4) }
}

/// A row of a card: its name, under it a line only where the name does
/// not say it all, and its control at the right. Every row is made here,
/// never laid out by hand.
fn line(title: SharedString, detail: Option<SharedString>, control: AnyElement, theme: &gpui_component::Theme) -> AnyElement {
    h_flex()
        .min_h(px(46.))
        .px(px(14.))
        .py(px(8.))
        .gap(px(16.))
        .items_center()
        .child(v_flex().flex_1().min_w_0().gap(px(1.)).child(div().text_size(px(13.)).child(title)).children(detail.map(|d| div().text_size(px(11.5)).line_height(px(15.)).text_color(theme.muted_foreground).child(d))))
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}

/// A row of a card on a sheet, where many stand in little room.
pub(crate) fn compact_line(title: SharedString, control: AnyElement) -> AnyElement {
    h_flex().min_h(px(40.)).px(px(14.)).py(px(5.)).gap(px(12.)).items_center().child(div().flex_1().min_w_0().text_size(px(13.)).child(title)).child(div().flex_shrink_0().child(control)).into_any_element()
}

impl Shell {
    pub(crate) fn render_settings(&mut self, focus: FocusHandle, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let section = self.settings_section;
        let rows = match section {
            "font" => self.fonts_rows(cx),
            "about" => self.about_rows(cx),
            "terminal" => self.terminal_rows(cx),
            "menubar" => self.menu_bar_rows(cx),
            _ => self.general_rows(cx),
        };
        let title = SECTIONS.iter().find(|(k, _, _)| *k == section).map(|(_, name, _)| t(name)).unwrap_or(t("General"));
        // At General's title's right, the window's language.
        let language = (section == "general").then(|| self.language_pill(cx));
        let pick = Rc::new(|this: &mut Self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>| this.settings_pick(what, id, window, cx));
        let close = Rc::new(|this: &mut Self, cx: &mut Context<Self>| {
            this.menus.close(cx);
            cx.notify();
        });
        let ours = |m: &crate::menu::Menu| menu_is_ours(m.what);
        let menu = if self.menus.open.as_ref().is_some_and(ours) || self.menus.gone.as_ref().is_some_and(|(m, _)| ours(m)) { self.menus.render(pick, close, cx) } else { None };
        let font_list = self.render_font_list(window, cx);
        // While the list stands over the page, a wheel anywhere else moves
        // nothing: the page under it does not scroll (`scroll.rs`).
        let list_box = self.font_list.is_some().then(|| self.font_list_box.clone());
        self.settings_focus = Some(focus.clone());
        div()
            .id("settings-window")
            .when_some(list_box, |d, b| {
                d.child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, window, _| {
                            let b = b.clone();
                            window.on_mouse_event(move |e: &ScrollWheelEvent, phase, _, cx| {
                                if phase == DispatchPhase::Capture && !b.get().contains(&e.position) {
                                    cx.stop_propagation();
                                }
                            });
                        },
                    )
                    .absolute()
                    .size_0(),
                )
            })
            .key_context(SETTINGS_CONTEXT)
            .track_focus(&focus)
            .size_full()
            .relative()
            .bg(theme.popover)
            .text_color(theme.foreground)
            .text_size(px(13.))
            // Escape puts away a list open here, and with none open closes
            // the window, as ⌘W does.
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.font_list.is_some() {
                    this.font_list_close(window, cx);
                } else if this.menus.open.is_some() {
                    this.menus.close(cx);
                    cx.notify();
                } else {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|_, _: &crate::CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &OpenSettings, window, _| window.activate_window()))
            .child(
                h_flex()
                    .size_full()
                    .items_stretch()
                    // A new language is faded into, rail and page alike.
                    .with_animation(ElementId::NamedInteger("settings-lang".into(), self.lang_serial as u64), Animation::new(Duration::from_millis(240)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
                    .child(self.settings_rail(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                drag_region(h_flex().h(TOP_H).flex_shrink_0().w_full().pl(PAGE_X).pr(PAGE_X + crate::csd::room(window, cx).1).items_center())
                                    .child(page_in(format!("title-{section}"), div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title)))
                                    .when_some(language, |d, l| d.child(div().flex_1()).child(l)),
                            )
                            // One pane: the settings have no gesture to route.
                            .child({
                                v_flex()
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .child(v_flex().id("settings-body").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.settings_scroll).px(PAGE_X).pt(px(2.)).pb(px(24.)).child(page_in(format!("page-{section}"), v_flex().gap(px(18.)).children(rows))))
                                    .vertical_scrollbar(&self.settings_scroll)
                            }),
                    ),
            )
            .children(menu)
            .children(font_list)
            .children(crate::csd::chrome(window, TOP_H / 2. - px(12.), |window, _| window.remove_window(), cx))
            .into_any_element()
    }

    /// The rail of sections, under the traffic lights: an icon and a name
    /// a section, the chosen one on a plate. A section with something
    /// waiting wears a dot in the accent. Its empty room drags the window.
    fn settings_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let current = self.settings_section;
        let waiting = self.update.found.is_some();
        drag_region(v_flex().w(RAIL_W).h_full().flex_shrink_0().pt(TOP_H).px(px(10.)).gap(px(2.)).bg(if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.5) }).border_r_1().border_color(theme.border))
            .children(SECTIONS.iter().map(|(key, label, icon)| {
                let active = *key == current;
                let key: &'static str = key;
                h_flex()
                    .id(SharedString::from(format!("settings-nav-{key}")))
                    .h(px(30.))
                    .px(px(10.))
                    .gap(px(9.))
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.sidebar_accent))
                    .when(!active, |d| d.hover(|s| s.bg(theme.sidebar_accent.opacity(0.6))))
                    // A press on a section is not a drag of the window.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.settings_section != key {
                            this.settings_section = key;
                            this.stop_recording(cx);
                            if key == "menubar" {
                                this.read_at_login(cx);
                            }
                            this.settings_scroll.set_offset(point(px(0.), px(0.)));
                            this.menus.close(cx);
                            this.save_section(window, cx);
                        }
                        cx.notify();
                    }))
                    .child(Icon::default().path(*icon).with_size(px(15.)).text_color(if active { theme.foreground } else { theme.muted_foreground }))
                    .child(div().flex_1().text_size(px(13.)).when(active, |d| d.font_weight(FontWeight::MEDIUM)).text_color(if active { theme.foreground } else { theme.sidebar_foreground }).child(t(label)))
                    .when(key == "about" && waiting, |d| d.child(div().size(px(7.)).rounded_full().bg(theme.primary)))
            }))
            .into_any_element()
    }

    /// General, as Emaki's Appearance: who you are, how the window looks,
    /// and how a note's text is sized and checked.
    fn general_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let theme_choice = div()
            .w(CONTROL_W)
            .child(self.motion.segmented(
                "appearance",
                vec![("system", "icons/device-desktop.svg", t("System").into()), ("light", "icons/sun.svg", t("Light").into()), ("dark", "icons/moon.svg", t("Dark").into())],
                match self.cfg.app.appearance.as_str() {
                    "light" => "light",
                    "dark" => "dark",
                    _ => "system",
                },
                true,
                Rc::new(|this: &mut Self, key, window, cx| {
                    this.cfg.app.appearance = key.to_string();
                    let _ = Config::edit(|c| c.app.appearance = key.to_string());
                    crate::look::apply(key, Some(window), cx);
                    cx.notify();
                }),
                cx,
            ))
            .into_any_element();
        // Accents are discs of the color, the chosen one with a ring
        // inside it in the card's ground: inside, so every disc is one
        // size and the first and last end where the controls do. The
        // chosen one's name is the row's line.
        let chosen = crate::look::accent(&self.cfg.app.accent).key;
        let ring = card_ground(&theme);
        let accents = h_flex()
            .w(CONTROL_W)
            .justify_between()
            .children(crate::look::ACCENTS.iter().map(|a| {
                let key = a.key;
                let active = key == chosen;
                let name: SharedString = t(a.name).into();
                let color = Rgba::try_from(crate::look::accent_hex(key, dark)).map(Hsla::from).unwrap_or(theme.primary);
                div()
                    .id(SharedString::from(format!("accent-{key}")))
                    .group("accent")
                    .size(px(24.))
                    .flex_shrink_0()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(color)
                    .managed_tooltip(move |window, cx| tip(name.clone(), window, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.cfg.app.accent = key.to_string();
                        let _ = Config::edit(|c| c.app.accent = key.to_string());
                        crate::look::install(key, cx);
                        let appearance = this.cfg.app.appearance.clone();
                        crate::look::apply(&appearance, Some(window), cx);
                        cx.notify();
                    }))
                    .child(div().size(px(16.)).rounded_full().border_2().border_color(if active { ring } else { ring.opacity(0.) }).when(!active, |d| d.group_hover("accent", |s| s.border_color(ring.opacity(0.55)))))
            }))
            .into_any_element();
        let accent_name: SharedString = t(crate::look::accent(&self.cfg.app.accent).name).into();
        // The picture at the start of the controls, what is done with it
        // after: a press on the picture chooses one too.
        let avatar = h_flex()
            .gap(px(8.))
            .items_center()
            .child(self.avatar(28., &theme).id("settings-avatar").cursor_pointer().hover(|s| s.opacity(0.85)).on_click(cx.listener(|this, _, _, cx| this.pick_avatar(cx))))
            .child(small_pill("avatar-choose", "icons/image.svg", t("Choose…"), Press::Quiet, false, &theme).h(CONTROL_H).on_click(cx.listener(|this, _, _, cx| this.pick_avatar(cx))))
            .when(!self.cfg.app.avatar.is_empty(), |d| d.child(small_pill("avatar-remove", "icons/trash.svg", t("Remove"), Press::Danger, false, &theme).h(CONTROL_H).on_click(cx.listener(|this, _, _, cx| this.set_avatar(String::new(), cx)))))
            .into_any_element();
        let text = vec![self.size_row(cx), self.spelling_row(cx)];
        vec![
            group(Some(t("You").into()), None, vec![line(t("Avatar").into(), None, avatar, &theme), line(t("Name").into(), None, self.name_field(cx), &theme)], &theme),
            group(Some(t("Container").into()), None, vec![self.container_row(cx)], &theme),
            group(Some(t("Window").into()), None, vec![line(t("Theme").into(), None, theme_choice, &theme), line(t("Accent").into(), Some(accent_name), accents, &theme)], &theme),
            group(Some(t("Text").into()), None, text, &theme),
        ]
    }

    /// The container's row: the card's one edge split in two by a
    /// hairline, the path at the left as a path reads (the code face,
    /// iCloud's cloud or a folder before it, the separators lighter than
    /// the names), a press on it showing it in the Finder; Change… at the
    /// right, the system's panel, answered in the main window, whose vault
    /// it closes if the vault is not in the new one. Each half lights whole under the pointer, rounded where it
    /// meets the card's corners.
    fn container_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let lit = theme.foreground.opacity(if theme.mode.is_dark() { 0.06 } else { 0.045 });
        let inner = CARD_R - px(1.);
        // With none, the row is the one thing to do: Choose Container…
        // whole, lit whole under the pointer, its keys at the right.
        let Some(dir) = self.container() else {
            return h_flex()
                .id("container-choose")
                .min_h(px(46.))
                .px(px(14.))
                .gap(px(9.))
                .items_center()
                .rounded(inner)
                .text_size(px(13.))
                .cursor_pointer()
                .hover(move |s| s.bg(lit))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|_, _, _, cx| {
                    let me = cx.entity();
                    let Some(main) = cx.try_global::<crate::MainShell>().map(|m| m.1) else { return };
                    cx.defer(move |cx| {
                        let _ = main.update(cx, |_, window, cx| me.update(cx, |s, cx| s.choose_container(window, cx)));
                    });
                }))
                .child(Icon::default().path("icons/folder-open.svg").with_size(px(15.)).text_color(theme.primary))
                .child(div().flex_1().child(t("Choose Container…")))
                .child(crate::widgets::kbd_hint("⌘⇧O", &theme))
                .into_any_element();
        };
        let path = {
            let shown = crate::sys::short_path(&dir);
            let icon = if shown.starts_with("iCloud Drive") { "icons/cloud.svg" } else { "icons/folder.svg" };
            let sep = std::path::MAIN_SEPARATOR;
            let mut parts: Vec<AnyElement> = Vec::new();
            for (i, part) in shown.split(sep).enumerate() {
                if i > 0 {
                    parts.push(div().px(px(3.)).text_color(theme.muted_foreground.opacity(0.7)).child(sep.to_string()).into_any_element());
                }
                parts.push(div().child(part.to_string()).into_any_element());
            }
            let words: SharedString = t("Reveal in Finder").into();
            // At the path's end, Close Container: the window as at its
            // first launch. Red under the pointer, as a thing let go.
            let (danger, wash) = (theme.danger, theme.danger.opacity(0.12));
            let close = div()
                .id("container-close")
                .size(px(24.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .text_color(theme.muted_foreground)
                .hover(move |s| s.bg(wash).text_color(danger))
                .managed_tooltip(|window, cx| tip(t("Close Container"), window, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.stop_propagation();
                    let me = cx.entity();
                    let Some(main) = cx.try_global::<crate::MainShell>().map(|m| m.1) else { return };
                    cx.defer(move |cx| {
                        let _ = main.update(cx, |_, window, cx| me.update(cx, |s, cx| s.close_container(window, cx)));
                    });
                }))
                .child(Icon::default().path("icons/close.svg").with_size(px(13.)));
            h_flex()
                .id("container-path")
                .size_full()
                .px(px(14.))
                .py(px(8.))
                .gap(px(9.))
                .items_center()
                .rounded_l(inner)
                .cursor_pointer()
                .hover(move |s| s.bg(lit))
                .managed_tooltip(move |window, cx| tip(words.clone(), window, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, _| crate::sys::reveal(&dir))
                .child(Icon::default().path(icon).with_size(px(15.)).text_color(theme.primary))
                .child(h_flex().flex_1().flex_wrap().min_w_0().font_family(self.look.mono_family.clone()).text_size(px(12.5)).text_color(theme.foreground).children(parts))
                .child(close)
                .into_any_element()
        };
        let change = h_flex()
            .id("container-change")
            .flex_shrink_0()
            .px(px(16.))
            .gap(px(7.))
            .items_center()
            .rounded_r(inner)
            .text_size(px(13.))
            .cursor_pointer()
            .hover(move |s| s.bg(lit))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|_, _, _, cx| {
                let me = cx.entity();
                let Some(main) = cx.try_global::<crate::MainShell>().map(|m| m.1) else { return };
                cx.defer(move |cx| {
                    let _ = main.update(cx, |_, window, cx| me.update(cx, |s, cx| s.choose_container(window, cx)));
                });
            }))
            .child(Icon::default().path("icons/folder-open.svg").with_size(px(15.)).text_color(theme.muted_foreground))
            .child(t("Change…"));
        h_flex()
            .min_h(px(46.))
            .items_stretch()
            .child(div().flex_1().min_w_0().flex().items_center().child(path))
            .child(div().w(px(1.)).flex_shrink_0().bg(theme.border))
            .child(change)
            .into_any_element()
    }

    /// The person's name, in a field the controls' width: on the page's
    /// own ground with an edge, so it reads as a place to type; the edge
    /// darker under the pointer, and the accent's with a soft ring of it
    /// while the field has the keyboard.
    fn name_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(input) = self.name_input.clone() else { return div().w(CONTROL_W).h(CONTROL_H).into_any_element() };
        let theme = cx.theme().clone();
        let edge = theme.foreground.opacity(if theme.mode.is_dark() { 0.16 } else { 0.14 });
        let hover = theme.foreground.opacity(0.28);
        let ring = theme.primary;
        let focus = input.read(cx).focus_handle(cx);
        div()
            .id("name-field")
            .track_focus(&focus)
            .w(CONTROL_W)
            .h(CONTROL_H)
            .px(px(9.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .border_1()
            .border_color(edge)
            .bg(theme.background)
            .cursor_text()
            .text_size(px(12.5))
            .hover(move |s| s.border_color(hover))
            .focus(move |s| s.border_color(ring).shadow(vec![BoxShadow { color: ring.opacity(0.22), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false }]))
            .child(Input::new(&input).appearance(false).bordered(false).px_0().on_secondary_click(Self::input_menu(&input, crate::shell::SETTINGS_EDIT, cx)))
            .into_any_element()
    }

    /// Font, as emaki's Fonts: the English font (every script but
    /// Chinese, Japanese and Korean), serifs over sans serifs with the
    /// person's own beside both as a tall tile; the CJK font; and the
    /// monospace font. Each face a tile drawn in itself, each row ending
    /// in a Custom tile that opens the list of the system's faces.
    fn fonts_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::fonts::{Slot, CUSTOM};
        let theme = cx.theme().clone();
        let e = self.cfg.editor.clone();
        let serif = crate::fonts::is_serif(&self.look.text_family);
        let tile_ground = if theme.mode.is_dark() { theme.muted } else { theme.popover };
        let tile = |id: SharedString, active: bool, family: SharedString, sample: SharedString, sample_px: f32, name: SharedString| {
            v_flex()
                .id(id)
                .flex_1()
                .min_w_0()
                .h(px(TILE_H))
                .px(px(10.))
                .justify_center()
                .gap(px(1.))
                .rounded(px(10.))
                .border_1()
                .cursor_pointer()
                .bg(tile_ground)
                .border_color(if active { theme.primary } else { theme.border })
                .when(active, |d| d.bg(theme.primary.opacity(0.08)))
                .when(!active, |d| d.hover(|s| s.border_color(theme.muted_foreground)))
                // One line, at a size that fits the narrowest tile.
                .child(div().whitespace_nowrap().overflow_hidden().text_size(px(sample_px)).line_height(px(24.)).font_family(family).child(sample))
                .child(div().truncate().text_size(px(11.5)).text_color(if active { theme.foreground } else { theme.muted_foreground }).child(name))
        };
        let english = |faces: &'static [crate::fonts::Face], cx: &mut Context<Self>| {
            h_flex().gap(px(TILE_GAP)).children(faces.iter().map(|face| {
                let key = face.key;
                tile(format!("face-{key}").into(), e.text_face == key, face.family.into(), "Ag".into(), 20., face.name.into()).on_click(cx.listener(move |this, _, _, cx| this.set_text_face(key, cx)))
            }))
        };
        let kind = |name: SharedString| div().text_size(px(11.5)).text_color(theme.muted_foreground).child(name);
        // Four tiles and the tall one as five columns of one width: the
        // rows' side grows four times the tall one's, from the gaps it has.
        let mut rows_side = v_flex().flex_basis(px(TILE_GAP * 3.)).min_w_0().gap(px(TILE_GAP));
        rows_side.style().flex_grow = Some(4.);
        let rows_side = rows_side.child(kind(t("Serif").into())).child(english(crate::fonts::SERIFS, cx)).child(div().h(px(4.))).child(kind(t("Sans serif").into())).child(english(crate::fonts::SANS, cx));
        // Its top level with the first row's, under a label as tall as
        // theirs that shows nothing.
        let mut tall = v_flex().flex_basis(px(0.)).min_w_0().gap(px(TILE_GAP));
        tall.style().flex_grow = Some(1.);
        let tall = tall.child(kind(t("Serif").into()).invisible()).child(div().flex_1().min_h_0().child(self.custom_tile(Slot::English, e.text_face == CUSTOM, true, cx)));
        let note = h_flex().p(px(TILE_PAD)).gap(px(TILE_GAP)).items_stretch().child(rows_side).child(tall).into_any_element();
        let cjk = h_flex()
            .p(px(TILE_PAD))
            .gap(px(TILE_GAP))
            .children(crate::fonts::CJK.iter().map(|face| {
                let key = face.key;
                let family = crate::fonts::cjk_family(key, serif);
                let (sample, name): (SharedString, SharedString) = match crate::fonts::cjk_native(key) {
                    Some(native) => (native.into(), face.name.into()),
                    None => ("汉字".into(), t("Auto").into()),
                };
                tile(format!("face-{key}").into(), e.cjk_face == key, family, sample, CJK_SAMPLE_PX, name).on_click(cx.listener(move |this, _, _, cx| this.set_cjk_face(key, cx)))
            }))
            .child(self.custom_tile(Slot::Cjk, e.cjk_face == CUSTOM, false, cx))
            .into_any_element();
        let mono = h_flex()
            .p(px(TILE_PAD))
            .gap(px(TILE_GAP))
            .children(crate::fonts::MONOS.iter().map(|face| {
                let key = face.key;
                tile(format!("face-{key}").into(), e.mono_face == key, face.family.into(), "fn(0) -> 1".into(), MONO_SAMPLE_PX, face.name.into()).on_click(cx.listener(move |this, _, _, cx| this.set_mono_face(key, cx)))
            }))
            .child(self.custom_tile(Slot::Mono, e.mono_face == CUSTOM, false, cx))
            .into_any_element();
        vec![group(Some(t("English font").into()), None, vec![note], &theme), group(Some(t("CJK font").into()), None, vec![cjk], &theme), group(Some(t("Monospace font").into()), None, vec![mono], &theme)]
    }

    /// The face the person chose for a Custom tile, empty while none was.
    fn custom_of(&self, slot: crate::fonts::Slot) -> String {
        let e = &self.cfg.editor;
        match slot {
            crate::fonts::Slot::English => e.text_custom.trim().to_string(),
            crate::fonts::Slot::Cjk => e.cjk_custom.trim().to_string(),
            crate::fonts::Slot::Mono => e.mono_custom.trim().to_string(),
        }
    }

    /// A Custom tile: the face chosen, drawn in it and named in its own
    /// writing and in English where it has both, else "Custom" with the
    /// row's sample in the window's face; a chevron says it opens a list.
    /// A press chooses its face where it has one and is not in use, else
    /// opens the list; a × at its corner, there while it has a face, puts
    /// the face away.
    fn custom_tile(&self, slot: crate::fonts::Slot, active: bool, tall: bool, cx: &mut Context<Self>) -> AnyElement {
        use crate::fonts::Slot;
        let theme = cx.theme().clone();
        let family = self.custom_of(slot);
        let none = family.is_empty();
        let native = self.font_families.as_ref().and_then(|f| f.native.get(&family).cloned());
        let open = self.font_list.is_some_and(|(s, _)| s == slot);
        let quiet = theme.muted_foreground;
        let (sample, sample_px): (SharedString, f32) = match slot {
            Slot::English => ("Ag".into(), if tall { 30. } else { 20. }),
            Slot::Cjk => (native.clone().unwrap_or_else(|| "汉字".into()).into(), CJK_SAMPLE_PX),
            Slot::Mono => ("fn(0) -> 1".into(), MONO_SAMPLE_PX),
        };
        let name: SharedString = if none {
            t("Custom").into()
        } else if slot == Slot::Cjk && native.is_some() {
            family.clone().into()
        } else {
            crate::fonts::both_names(native.as_deref(), &family).into()
        };
        let face: SharedString = if none { theme.font_family.clone() } else { family.clone().into() };
        let ground = if theme.mode.is_dark() { theme.muted } else { theme.popover };
        let lit = active || open;
        let clear_key = format!("font-clear-{slot:?}");
        let moving = self.motion.presence(&clear_key, !none, cx);
        let clear = (!none || moving.is_some()).then(|| {
            let over = if theme.mode.is_dark() { theme.foreground.opacity(0.12) } else { theme.muted };
            let ink = theme.foreground;
            let shown = moving.is_none_or(|(on, _)| on);
            div()
                .id(SharedString::from(format!("{clear_key}-x")))
                .absolute()
                .top(px(3.))
                .right(px(3.))
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .text_color(quiet)
                .hover(move |s| s.bg(over).text_color(ink))
                .managed_tooltip(|window, cx| tip(t("Clear"), window, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    swallow_click(window, cx);
                    if !none {
                        this.clear_custom_font(slot, cx);
                    }
                }))
                .child(Icon::default().path("icons/close.svg").with_size(px(10.)))
                .with_animation(ElementId::Name(format!("{clear_key}-{}", moving.map(|(_, n)| n).unwrap_or(0)).into()), Animation::new(crate::widgets::FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if moving.is_none() { 1. } else if shown { t } else { 1. - t }))
        });
        let mark = Icon::default().path("icons/chevrons-up-down.svg").with_size(px(12.)).text_color(quiet);
        let tiles = self.font_tiles.clone();
        v_flex()
            .id(SharedString::from(format!("face-custom-{slot:?}")))
            .relative()
            .when(tall, |d| d.size_full())
            .when(!tall, |d| d.flex_1().h(px(TILE_H)))
            .min_w_0()
            .px(px(10.))
            .justify_center()
            .gap(px(if tall { 4. } else { 1. }))
            .rounded(px(10.))
            .border_1()
            .cursor_pointer()
            .bg(ground)
            .border_color(if lit { theme.primary } else { theme.border })
            .when(lit, |d| d.bg(theme.primary.opacity(0.08)))
            .when(!lit, |d| d.hover(move |s| s.border_color(quiet)))
            .child(div().whitespace_nowrap().overflow_hidden().text_size(px(sample_px)).line_height(px(if tall { 38. } else { 24. })).font_family(face).when(none, |d| d.text_color(quiet)).child(sample))
            .child(h_flex().gap(px(2.)).items_center().child(div().flex_1().min_w_0().truncate().text_size(px(11.5)).text_color(if active { theme.foreground } else { quiet }).child(name)).child(mark))
            .when(!none, |d| {
                let full: SharedString = crate::fonts::both_names(native.as_deref(), &family).into();
                d.managed_tooltip(move |window, cx| tip(full.clone(), window, cx))
            })
            .children(clear)
            .child(canvas(move |b, _, _| { tiles.borrow_mut().insert(slot, b); }, |_, _, _, _| {}).absolute().inset_0())
            .on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                if !none && !active {
                    this.set_custom_key(slot, cx);
                } else {
                    this.font_list_open(slot, window, cx);
                }
            }))
            .into_any_element()
    }

    /// The name's field, made with the settings window: the account's
    /// first name stands in it as a placeholder, and a name typed takes
    /// its place everywhere the window says it.
    pub(crate) fn make_name_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let machine = crate::sys::user_first_name();
        let saved = self.cfg.app.user_name.clone();
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(if machine.is_empty() { t("Your name").to_string() } else { machine });
            s.set_value(saved, window, cx);
            s
        });
        cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                let name = input.read(cx).value().to_string();
                this.set_user_name(name, cx);
            }
        })
        .detach();
        self.name_input = Some(input);
    }

    /// The search field of the list of the system's faces, made with the
    /// settings window: a change puts ↑ and ↓ at the top, ↩ chooses.
    pub(crate) fn make_font_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(t("Search fonts")));
        cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => {
                if let Some((_, at)) = this.font_list.as_mut() {
                    *at = 0;
                    this.font_scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }
            }
            InputEvent::PressEnter { .. } => this.font_list_choose(None, window, cx),
            _ => {}
        })
        .detach();
        self.font_query = Some(input);
    }

    /// The families the list shows for its tile: those the search's words
    /// all find, by either name.
    fn font_list_rows(&self, cx: &App) -> Rc<Vec<String>> {
        let (Some((slot, _)), Some(families)) = (self.font_list, self.font_families.as_ref()) else { return Rc::default() };
        let query = self.font_query.as_ref().map(|q| q.read(cx).value().to_string()).unwrap_or_default();
        let count = families.of(slot).len();
        if let Some((s, q, n, rows)) = self.font_rows.borrow().as_ref() {
            if *s == slot && *q == query && *n == count {
                return rows.clone();
            }
        }
        let rows = Rc::new(families.matching(slot, &query));
        *self.font_rows.borrow_mut() = Some((slot, query, count, rows.clone()));
        rows
    }

    /// The list of the system's faces for a tile, its search empty and its
    /// keyboard there, the face chosen in view. The families are read the
    /// first time, off the main thread.
    fn font_list_open(&mut self, slot: crate::fonts::Slot, window: &mut Window, cx: &mut Context<Self>) {
        self.menus.close(cx);
        self.font_list = Some((slot, 0));
        if let Some(query) = self.font_query.clone() {
            query.update(cx, |s, cx| s.set_value("", window, cx));
            window.focus(&query.read(cx).focus_handle(cx), cx);
        }
        self.font_list_show_chosen(cx);
        if self.font_families.is_none() && !self.font_families_reading {
            self.font_families_reading = true;
            #[cfg(target_os = "macos")]
            let read = cx.background_spawn(async move { crate::fonts::system_families() });
            #[cfg(not(target_os = "macos"))]
            let read = {
                let names = cx.text_system().all_font_names();
                cx.background_spawn(async move { crate::fonts::system_families_of(names) })
            };
            cx.spawn(async move |this, cx| {
                let families = read.await;
                let _ = this.update(cx, |this, cx| {
                    this.font_families_reading = false;
                    this.font_families = Some(Rc::new(families));
                    this.font_list_show_chosen(cx);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// For a probe: the list for a tile, with words in its search.
    pub(crate) fn probe_font_list(&mut self, slot: crate::fonts::Slot, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.font_list_open(slot, window, cx);
        if let Some(q) = self.font_query.clone() {
            q.update(cx, |s, cx| s.set_value(words.to_string(), window, cx));
        }
    }

    /// ↑ and ↓ on the face chosen, a few rows down from the list's top.
    fn font_list_show_chosen(&mut self, cx: &App) {
        let Some((slot, _)) = self.font_list else { return };
        let chosen = self.custom_of(slot);
        let rows = self.font_list_rows(cx);
        let at = rows.iter().position(|f| *f == chosen).unwrap_or(0);
        self.font_list = Some((slot, at));
        self.font_scroll.set_offset(point(px(0.), px(-(at.saturating_sub(3) as f32 * FONT_ROW_H))));
    }

    fn font_list_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.font_list = None;
        if let Some(focus) = self.settings_focus.clone() {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// The row chosen, or the one ↑ and ↓ are on: the tile's face, and in use.
    fn font_list_choose(&mut self, ix: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((slot, at)) = self.font_list else { return };
        let rows = self.font_list_rows(cx);
        let Some(family) = rows.get(ix.unwrap_or(at)).cloned() else { return };
        self.font_list_close(window, cx);
        let e = &mut self.cfg.editor;
        match slot {
            crate::fonts::Slot::English => e.text_custom = family,
            crate::fonts::Slot::Cjk => e.cjk_custom = family,
            crate::fonts::Slot::Mono => e.mono_custom = family,
        }
        self.set_custom_key(slot, cx);
    }

    /// ↑ and ↓ in the list's field move through what it shows, the row in view.
    fn font_list_step(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.font_list_rows(cx).len();
        let Some((_, at)) = self.font_list.as_mut() else { return };
        if n == 0 {
            return;
        }
        *at = (*at as isize + by).clamp(0, n as isize - 1) as usize;
        let (top, view) = (-f32::from(self.font_scroll.offset().y), f32::from(self.font_scroll.bounds().size.height));
        let y = *at as f32 * FONT_ROW_H;
        if y < top {
            self.font_scroll.set_offset(point(px(0.), px(-y)));
        } else if y + FONT_ROW_H > top + view && view > 0. {
            self.font_scroll.set_offset(point(px(0.), px(-(y + FONT_ROW_H - view))));
        }
        cx.notify();
    }

    /// A Custom tile's face in use.
    fn set_custom_key(&mut self, slot: crate::fonts::Slot, cx: &mut Context<Self>) {
        let e = &mut self.cfg.editor;
        let key = crate::fonts::CUSTOM.to_string();
        match slot {
            crate::fonts::Slot::English => e.text_face = key,
            crate::fonts::Slot::Cjk => e.cjk_face = key,
            crate::fonts::Slot::Mono => e.mono_face = key,
        }
        self.save_editor(cx);
    }

    /// The ×: the tile's face put away, and where it was in use, the row's
    /// first face in its place.
    fn clear_custom_font(&mut self, slot: crate::fonts::Slot, cx: &mut Context<Self>) {
        let e = &mut self.cfg.editor;
        let custom = crate::fonts::CUSTOM;
        match slot {
            crate::fonts::Slot::English => {
                e.text_custom.clear();
                if e.text_face == custom {
                    e.text_face = crate::fonts::SANS[0].key.into();
                }
            }
            crate::fonts::Slot::Cjk => {
                e.cjk_custom.clear();
                if e.cjk_face == custom {
                    e.cjk_face = crate::fonts::CJK[0].key.into();
                }
            }
            crate::fonts::Slot::Mono => {
                e.mono_custom.clear();
                if e.mono_face == custom {
                    e.mono_face = crate::fonts::MONOS[0].key.into();
                }
            }
        }
        self.save_editor(cx);
    }

    /// The list of the system's faces, hanging off its tile: a search
    /// field over the families, each named in its own writing and in
    /// English where it has both, the one chosen ticked. Only the rows in
    /// view are drawn, each in the window's face: drawn in its own, every
    /// face scrolled past would be loaded and kept.
    fn render_font_list(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (slot, at) = self.font_list?;
        let theme = cx.theme().clone();
        let view = window.viewport_size();
        let by = self.font_tiles.borrow().get(&slot).copied().unwrap_or_default();
        let rows = self.font_list_rows(cx);
        let chosen = self.custom_of(slot);
        let (w, gap) = (px(320.), px(6.));
        let left = by.left().min(view.width - px(12.) - w).max(px(12.));
        let below = view.height - by.bottom() - gap - px(12.);
        let above = by.top() - gap - px(12.);
        let up = below < px(260.) && above > below;
        let most = if up { above } else { below }.min(px(380.));
        let list_h = f32::from((most - px(48.)).max(px(FONT_ROW_H * 3.)));
        let field = self.font_query.as_ref().map(|q| {
            h_flex()
                .mx(px(8.))
                .mb(px(6.))
                .h(px(30.))
                .px(px(8.))
                .gap(px(6.))
                .items_center()
                .rounded(px(8.))
                .bg(theme.muted)
                // Taken before the field, whose own ↑ and ↓ go to its
                // start and end.
                .capture_action(cx.listener(|this, _: &gpui_component::input::MoveUp, _, cx| {
                    this.font_list_step(-1, cx);
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &gpui_component::input::MoveDown, _, cx| {
                    this.font_list_step(1, cx);
                    cx.stop_propagation();
                }))
                .child(Icon::default().path("icons/search.svg").with_size(px(13.)).text_color(theme.muted_foreground))
                .child(div().flex_1().min_w_0().track_focus(&q.read(cx).focus_handle(cx)).child(Input::new(q).appearance(false).bordered(false).px_0()))
        });
        let quiet = |words: &str| div().px(px(14.)).py(px(10.)).text_size(px(12.5)).text_color(theme.muted_foreground).child(words.to_string()).into_any_element();
        let body = match &self.font_families {
            None => quiet(t("Reading fonts…")),
            Some(_) if rows.is_empty() => quiet(t("No font matches")),
            Some(families) => {
                let total = rows.len() as f32 * FONT_ROW_H;
                let height = total.min(list_h);
                let top = -f32::from(self.font_scroll.offset().y);
                let first = (top / FONT_ROW_H).floor().max(0.) as usize;
                let last = (((top + height) / FONT_ROW_H).ceil() as usize + 1).min(rows.len());
                let dark = theme.mode.is_dark();
                let lines: Vec<AnyElement> = (first..last)
                    .map(|ix| {
                        let family = rows[ix].clone();
                        let on = family == chosen;
                        let over = if dark { theme.foreground.opacity(0.06) } else { theme.muted };
                        h_flex()
                            .id(("font-row", ix))
                            .h(px(FONT_ROW_H))
                            .flex_shrink_0()
                            .px(px(10.))
                            .gap(px(6.))
                            .items_center()
                            .rounded(px(7.))
                            .cursor_pointer()
                            .text_size(px(13.))
                            .when(ix == at, |d| d.bg(if dark { theme.foreground.opacity(0.11) } else { theme.list_hover }))
                            .when(ix != at, |d| d.hover(move |s| s.bg(over)))
                            .child(div().flex_1().min_w_0().truncate().child(families.shown(&family)))
                            .when(on, |d| d.child(Icon::new(gpui_component::IconName::Check).with_size(px(12.)).text_color(theme.primary)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.font_list_choose(Some(ix), window, cx);
                            }))
                            .into_any_element()
                    })
                    .collect();
                let boxed = self.font_list_box.clone();
                div()
                    .relative()
                    .h(px(height))
                    .child(
                        div()
                            .id("font-list-rows")
                            .size_full()
                            .px(px(5.))
                            .overflow_y_scroll()
                            .track_scroll(&self.font_scroll)
                            .child(v_flex().child(div().h(px(first as f32 * FONT_ROW_H)).flex_shrink_0()).children(lines).child(div().h(px((rows.len() - last) as f32 * FONT_ROW_H)).flex_shrink_0())),
                    )
                    .child(canvas(move |b, _, _| boxed.set(b), |_, _, _, _| {}).absolute().inset_0())
                    .vertical_scrollbar(&self.font_scroll)
                    .into_any_element()
            }
        };
        let card = v_flex()
            .id("font-list")
            .occlude()
            .absolute()
            .left(left)
            .when(up, |d| d.bottom(view.height - by.top() + gap))
            .when(!up, |d| d.top(by.bottom() + gap))
            .w(w)
            .pt(px(8.))
            .pb(px(6.))
            .rounded(px(12.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow(crate::widgets::float_shadow(&theme))
            .children(field)
            .child(body)
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.font_list_close(window, cx)));
        Some(crate::widgets::fade_in("font-list-in", card).into_any_element())
    }

    /// General, Text's sizes: a preset for both places at once, and each
    /// place's size under it, which a preset sets. Custom is the person's
    /// own, set here by a step, and kept while a preset is chosen; under a
    /// preset the sizes are its and are only shown.
    /// The words' size and code's, a stepper each on one row, with a
    /// button back to Medium's beside them once either left it.
    fn size_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let sizes = self.cfg.editor.text_sizes();
        let place = |this: &Self, key: &'static str, name: &str, value: f32, cx: &mut Context<Self>| h_flex().gap(px(8.)).items_center().child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child(name.to_string())).child(this.stepper(key, value, true, cx));
        let control = h_flex()
            .gap(px(16.))
            .items_center()
            .children(self.size_reset(sizes, cx))
            .child(place(self, "note", t("Text"), sizes.note, cx))
            .child(place(self, "code", t("Code"), sizes.code, cx))
            .into_any_element();
        line(t("Size").into(), None, control, &theme)
    }

    /// Beside the sizes once either is not Medium's: a button that puts
    /// both back. It comes and goes in motion (`presence`).
    fn size_reset(&self, sizes: onyx_core::config::TextSizes, cx: &mut Context<Self>) -> Option<AnyElement> {
        use onyx_core::config::TextSizes;
        let shown = sizes != TextSizes::MEDIUM;
        let id = "size-reset";
        let moving = self.motion.presence(id, shown, cx);
        if !shown && moving.is_none() {
            return None;
        }
        let theme = cx.theme().clone();
        let hover = theme.foreground.opacity(0.07);
        let tip: SharedString = tf("Reset to {size} pt", &[("size", &format!("{} / {}", size_words(TextSizes::MEDIUM.note), size_words(TextSizes::MEDIUM.code)))]).into();
        let button = div()
            .id(id)
            .size(px(26.))
            .flex_shrink_0()
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .when(shown, |d| {
                d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |this, _, window, cx| {
                    crate::widgets::swallow_click(window, cx);
                    this.set_sizes(|s| *s = TextSizes::MEDIUM, cx);
                }))
            })
            .managed_tooltip(move |window, cx| crate::widgets::tip(tip.clone(), window, cx))
            .child(Icon::default().path("icons/rotate.svg").with_size(px(14.)).text_color(theme.muted_foreground));
        Some(match moving {
            Some((coming, n)) => button.with_animation(ElementId::Name(format!("{id}-{n}").into()), Animation::new(crate::widgets::FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
            None => button.into_any_element(),
        })
    }

    /// A size in points: a step down, the number with "pt" after it, and
    /// a step up, on the track a choice has. A step is a whole point.
    /// Not `live`, it shows a preset's size and nothing in it moves it.
    fn stepper(&self, key: &'static str, value: f32, live: bool, cx: &mut Context<Self>) -> AnyElement {
        use onyx_core::config::TextSizes;
        let (down, up) = (TextSizes::step(value, false), TextSizes::step(value, true));
        self.stepper_of(key, value, live, down, up, Rc::new(move |this: &mut Self, to, cx| this.set_sizes(|s| if key == "code" { s.code = to } else { s.note = to }, cx)), cx)
    }

    /// A size with a press down and a press up, for a sheet.
    pub(crate) fn stepper_with(&self, key: &'static str, value: f32, down: f32, up: f32, set: Rc<dyn Fn(&mut Self, f32, &mut Context<Self>)>, cx: &mut Context<Self>) -> AnyElement {
        self.stepper_of(key, value, true, down, up, set, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn stepper_of(&self, key: &'static str, value: f32, live: bool, down: f32, up: f32, set: Rc<dyn Fn(&mut Self, f32, &mut Context<Self>)>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let track = if dark { theme.tab_bar_segmented } else { theme.muted };
        let plate = if dark { theme.secondary_active } else { theme.popover };
        let step = |up: bool, to: f32, icon: &'static str, cx: &mut Context<Self>| {
            let able = live && to != value;
            let set = set.clone();
            div()
                .id(SharedString::from(format!("size-{key}-{}", if up { "up" } else { "down" })))
                .size(px(22.))
                .flex_shrink_0()
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .when(able, |d| {
                    d.cursor_pointer().hover(move |s| s.bg(plate).shadow_sm()).on_click(cx.listener(move |this, _, window, cx| {
                        crate::widgets::swallow_click(window, cx);
                        set(this, to, cx);
                    }))
                })
                .child(Icon::default().path(icon).with_size(px(13.)).text_color(if able { theme.foreground } else { theme.muted_foreground.opacity(0.5) }))
        };
        let down = step(false, down, "icons/minus.svg", cx);
        let up = step(true, up, "icons/plus.svg", cx);
        h_flex()
            .w(px(140.))
            .h(CONTROL_H)
            .p(px(3.))
            .gap(px(4.))
            .rounded(px(8.))
            .bg(track)
            .items_center()
            .child(down)
            .child(h_flex().flex_1().justify_center().items_center().gap(px(3.)).child(div().w(px(42.)).text_center().font_weight(FontWeight::MEDIUM).text_size(px(13.)).text_color(if live { theme.foreground } else { theme.muted_foreground }).child(size_words(value))).child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child("pt")))
            .child(up)
            .into_any_element()
    }

    /// For a probe: the CJK face by its key, as its tile sets it.
    pub(crate) fn probe_cjk_face(&mut self, key: String, cx: &mut Context<Self>) {
        self.cfg.editor.cjk_face = key;
        self.save_editor(cx);
    }

    /// A carried face chosen; the Custom tile keeps its own for later.
    fn set_text_face(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.cfg.editor.text_face = key.to_string();
        self.save_editor(cx);
    }

    fn set_mono_face(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.cfg.editor.mono_face = key.to_string();
        self.save_editor(cx);
    }

    fn set_cjk_face(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.cfg.editor.cjk_face = key.to_string();
        self.save_editor(cx);
    }

    /// The editor's settings written, and every editor told.
    fn save_editor(&mut self, cx: &mut Context<Self>) {
        let e = self.cfg.editor.clone();
        let _ = Config::edit(|c| c.editor = e);
        self.push_look(cx);
        cx.notify();
    }

    /// A size changed.
    fn set_sizes(&mut self, change: impl FnOnce(&mut onyx_core::config::TextSizes), cx: &mut Context<Self>) {
        change(&mut self.cfg.editor.size);
        self.cfg.editor.size = self.cfg.editor.size.clamped();
        self.save_editor(cx);
    }

    /// About, the same in every product: the title card, set as a film's
    /// first one is, the pills to the source and the notes, and the updates.
    fn about_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let hover = theme.link;
        let studio = div()
            .id("studio-link")
            .cursor_pointer()
            .text_color(theme.foreground)
            .text_decoration_1()
            .text_decoration_color(theme.foreground.opacity(0.35))
            .hover(move |s| s.text_color(hover).text_decoration_color(hover))
            .on_click(|_, _, _| crate::sys::open("https://github.com/alkaline-software"))
            .child("Alkaline Software")
            .into_any_element();
        let title = v_flex()
            .w_full()
            .items_center()
            .pt(px(30.))
            .pb(px(28.))
            .child(weave(&t("{studio} presents"), vec![("studio", studio)], h_flex().text_size(px(14.)).line_height(px(22.)).text_color(theme.muted_foreground).gap(px(0.))))
            .child(img("icon/icon-512.png").mt(px(18.)).size(px(88.)))
            // The row's height is its own, whatever face or marks the name holds.
            .child(div().mt(px(6.)).child(wordmark("Onyx", 42., theme.primary, cx).line_height(px(48.))))
            .child(div().mt(px(4.)).text_size(px(12.5)).text_color(theme.muted_foreground).child(tf("Version {version}", &[("version", &update::current_version())])));
        let pills = h_flex()
            .w_full()
            .justify_center()
            .pt(px(4.))
            .gap(px(8.))
            .child(small_pill("about-source", "icons/github.svg", "GitHub", Press::Quiet, false, &theme).h(px(28.)).on_click(|_, _, _| crate::sys::open(&update::repo_page())))
            .child(small_pill("about-notes", "icons/file-text.svg", t("Release Notes"), Press::Quiet, false, &theme).h(px(28.)).on_click(|_, _, _| crate::sys::open(&update::release_page(update::current_version()))));
        let u = self.update.clone();
        let (icon, words, kind) = match (&u.found, u.downloading) {
            (_, Some(p)) => ("icons/download.svg", tf("Downloading… {percent}%", &[("percent", &p)]), Press::Primary),
            (Some(v), None) => ("icons/download.svg", tf("Update to {version}", &[("version", v)]), Press::Primary),
            (None, None) => ("icons/refresh.svg", t("Check for updates").to_string(), Press::Quiet),
        };
        let busy = u.checking || u.downloading.is_some();
        let button = small_pill("update", icon, words, kind, busy, &theme).h(CONTROL_H).min_w(px(190.)).justify_center().when(!busy, |d| {
            d.on_click(cx.listener(|this, _, _, cx| {
                if this.update.found.is_some() {
                    this.install_update(cx);
                } else {
                    this.check_updates(cx);
                }
            }))
        });
        let said = u.said.clone().map(SharedString::from);
        let check = self.motion.switch(
            "check-updates",
            self.cfg.app.check_updates,
            Rc::new(|this: &mut Self, on, _, cx| {
                this.cfg.app.check_updates = on;
                let _ = Config::edit(|c| c.app.check_updates = on);
                cx.notify();
            }),
            cx,
        );
        let mut rows = vec![group(None, None, vec![title.into_any_element()], &theme), pills.into_any_element()];
        if !update::from_store() {
            rows.push(div().w_full().pt(px(12.)).child(group(None, None, vec![line(t("Updates").into(), said, button.into_any_element(), &theme), line(t("Check automatically").into(), None, check, &theme)], &theme)).into_any_element());
        }
        rows
    }

    /// The window's language, at the right of General's name: the
    /// system's, or a language by its flag and letters, in a list that
    /// drops down.
    fn language_pill(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let chosen = crate::i18n::chosen(&self.cfg.app.language);
        let (icon, label, picture) = match crate::i18n::CHOICES.iter().find(|(c, _, _)| *c == chosen) {
            Some((_, flag, letters)) => (*flag, letters.to_string(), true),
            None => ("icons/globe.svg", t("System").to_string(), false),
        };
        h_flex()
            .id("language-pill")
            .h(CONTROL_H)
            .px(px(12.))
            .gap(px(7.))
            .items_center()
            .rounded_full()
            .border_1()
            .border_color(theme.foreground.opacity(0.16))
            .text_size(px(12.5))
            .cursor_pointer()
            .hover(|s| s.bg(theme.foreground.opacity(0.06)))
            // A press on it is not a drag of the window's top.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                let at = ev.position() + point(px(-60.), px(16.));
                let chosen = crate::i18n::chosen(&this.cfg.app.language);
                let mut entries = vec![Entry::Choice { id: "system".into(), icon: "icons/globe.svg", label: t("System").into(), picture: false, chosen: chosen == "system" }, Entry::Rule];
                entries.extend(crate::i18n::CHOICES.iter().map(|(code, flag, letters)| Entry::Choice { id: (*code).into(), icon: flag, label: (*letters).into(), picture: true, chosen: chosen == *code }));
                this.menus.show("language", at, entries);
                cx.notify();
            }))
            .map(|d| if picture { d.child(img(icon).w(px(17.)).h(px(12.))) } else { d.child(Icon::default().path(icon).with_size(px(14.))) })
            .child(label)
            .child(Icon::default().path("icons/chevron-down.svg").with_size(px(12.)).text_color(theme.muted_foreground))
            .into_any_element()
    }

    /// General, Text's spelling and grammar: what it reads in, and the
    /// check on or off, on one row.
    /// English is read here, for grammar too; the others are the system's
    /// spelling (`docs/editor.md`).
    fn spelling_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let on = self.cfg.editor.check_writing;
        let check = self.motion.switch(
            "check-writing",
            on,
            Rc::new(|this: &mut Self, on, _, cx| {
                this.cfg.editor.check_writing = on;
                let _ = Config::edit(|c| c.editor.check_writing = on);
                this.push_writing(cx);
                cx.notify();
            }),
            cx,
        );
        let chosen = self.cfg.editor.writing_language.clone();
        let (_, flag, name) = WRITING.iter().find(|(key, _, _)| *key == chosen).copied().unwrap_or(WRITING[0]);
        let pill = h_flex()
            .id("writing-pill")
            .h(CONTROL_H)
            .px(px(12.))
            .gap(px(7.))
            .items_center()
            .rounded_full()
            .border_1()
            .border_color(theme.foreground.opacity(0.16))
            .text_size(px(12.5))
            .cursor_pointer()
            .hover(|s| s.bg(theme.foreground.opacity(0.06)))
            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                let at = ev.position() + point(px(-60.), px(16.));
                let chosen = this.cfg.editor.writing_language.clone();
                let entries = WRITING.iter().map(|(key, flag, name)| Entry::Choice { id: (*key).into(), icon: flag, label: t(name).into(), picture: true, chosen: chosen == *key }).collect();
                this.menus.show("writing-language", at, entries);
                cx.notify();
            }))
            .child(img(flag).w(px(17.)).h(px(12.)))
            .child(t(name))
            .child(Icon::default().path("icons/chevron-down.svg").with_size(px(12.)).text_color(theme.muted_foreground))
            .into_any_element();
        let language = div().when(!on, |d| d.opacity(0.45).occlude()).child(pill);
        line(t("Check spelling and grammar").into(), None, h_flex().gap(px(12.)).items_center().child(language).child(check).into_any_element(), &theme)
    }

    /// The terminal's look and behavior, as Emaki's Settings, Terminal
    /// has them: its scheme, its cursor, and the face, which is the
    /// window's code face, set in Font.
    fn terminal_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let term = self.cfg.terminal.clone();
        let pick = |keys: &[&'static str], now: &str| keys.iter().copied().find(|k| *k == now).unwrap_or(keys[0]);
        let scheme = div().w(CONTROL_W).child(self.motion.segmented("term-theme", vec![("match", "", t("Match window").into()), ("dark", "", t("Dark").into()), ("light", "", t("Light").into())], pick(&["match", "dark", "light"], &term.theme), true, Rc::new(|this: &mut Self, key, _, cx| this.set_terminal(|t| t.theme = key.into(), cx)), cx)).into_any_element();
        let cursor = div().w(CONTROL_W).child(self.motion.segmented("term-cursor", vec![("bar", "", t("Bar").into()), ("block", "", t("Block").into()), ("underline", "", t("Underline").into())], pick(&["bar", "block", "underline"], &term.cursor), true, Rc::new(|this: &mut Self, key, _, cx| this.set_terminal(|t| t.cursor = key.into(), cx)), cx)).into_any_element();
        let blink = self.motion.switch("term-blink", term.cursor_blink, Rc::new(|this: &mut Self, on, _, cx| this.set_terminal(|t| t.cursor_blink = on, cx)), cx);
        let ligatures = self.motion.switch("term-ligatures", term.ligatures, Rc::new(|this: &mut Self, on, _, cx| this.set_terminal(|t| t.ligatures = on, cx)), cx);
        let copy = self.motion.switch("term-copy", term.copy_on_select, Rc::new(|this: &mut Self, on, _, cx| this.set_terminal(|t| t.copy_on_select = on, cx)), cx);
        // The face is the window's code face, set in Font.
        let fonts = choice_pill("term-fonts", "icons/a-large-small.svg", t("Open Fonts"), Press::Quiet, false, &theme)
            .h(CONTROL_H)
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_section = "font";
                cx.notify();
            }))
            .into_any_element();
        // The marks it joins, with a zero-width mark between the two
        // letters of each, or the window's own face would join them here.
        let joins: SharedString = tf("Joins {arrow} and {unequal} into one mark.", &[("arrow", &"-\u{200C}>"), ("unequal", &"!\u{200C}=")]).into();
        let (link_color, link_hover) = (theme.muted_foreground, theme.primary);
        let credit_link = |id: &'static str, name: &'static str, url: &'static str| {
            div()
                .id(id)
                .cursor_pointer()
                .text_decoration_1()
                .text_decoration_color(link_color.opacity(0.6))
                .hover(move |s| s.text_color(link_hover).text_decoration_color(link_hover))
                .on_click(move |_, _, cx| cx.open_url(url))
                .child(name)
        };
        // The two names are links, set where each language's order puts
        // them.
        let credit = weave(
            t("The styling and UI design are from {kaku}, made by {tw93}."),
            vec![("kaku", credit_link("term-credit", "Kaku", "https://kaku.fun").into_any_element()), ("tw93", credit_link("term-credit-author", "tw93", "https://github.com/tw93").into_any_element())],
            h_flex().flex_wrap().px(px(14.)).text_size(px(11.5)).text_color(theme.muted_foreground),
        )
        .into_any_element();
        vec![
            group(Some(t("Look").into()), None, vec![line(t("Theme").into(), None, scheme, &theme), line(t("Cursor").into(), None, cursor, &theme), line(t("Blinking cursor").into(), None, blink, &theme), line(t("Font").into(), None, fonts, &theme)], &theme),
            group(Some(t("Behavior").into()), None, vec![line(t("Ligatures").into(), Some(joins), ligatures, &theme), line(t("Copy on select").into(), None, copy, &theme)], &theme),
            credit,
        ]
    }

    /// Settings, Menu Bar: the item, the shortcut that opens and closes the
    /// note from any app, recorded as Mado records one, and Onyx at login.
    fn menu_bar_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let bar = self.cfg.menu_bar.clone();
        let shown = self.motion.switch("bar-shown", bar.shown, Rc::new(|this: &mut Self, on, _, cx| {
            this.cfg.menu_bar.shown = on;
            let _ = Config::edit(|c| c.menu_bar.shown = on);
            crate::sys::set_menu_bar_shown(on);
            // Hidden, its note is still the shortcut's.
            cx.notify();
        }), cx);
        let login = self.motion.switch("bar-login", self.at_login, Rc::new(|this: &mut Self, on, _, cx| this.set_at_login(on, cx)), cx);
        // A line under Shortcut only while there is something to say: a
        // shortcut another app holds, or keys that cannot be one.
        let said = self.record_said.clone();
        vec![group(None, None, vec![line(t("Show in menu bar").into(), None, shown, &theme), line(t("Shortcut").into(), said, self.shortcut_field(cx), &theme), line(t("Open at login").into(), None, login, &theme)], &theme)]
    }

    /// Login Items as the system has it, asked off the window's thread:
    /// the answer takes a fifth of a second in a bundle, and asked as
    /// Menu Bar was chosen it held the window that long.
    pub(crate) fn read_at_login(&mut self, cx: &mut Context<Self>) {
        let read = cx.background_spawn(async { crate::sys::launches_at_login() });
        cx.spawn(async move |this, cx| {
            let on = read.await;
            let _ = this.update(cx, |this, cx| {
                if this.at_login != on {
                    this.at_login = on;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Onyx put in Login Items or taken out: the switch moves at once, and
    /// the system's answer, off the window's thread, has the last word.
    fn set_at_login(&mut self, on: bool, cx: &mut Context<Self>) {
        self.at_login = on;
        cx.notify();
        let done = cx.background_spawn(async move { crate::sys::set_launch_at_login(on).map(|()| crate::sys::launches_at_login()) });
        cx.spawn(async move |this, cx| {
            let done = done.await;
            let _ = this.update(cx, |this, cx| {
                match done {
                    Ok(now) => this.at_login = now,
                    Err(e) => {
                        this.at_login = !on;
                        this.say(tf("Login Items did not take Onyx: {why}", &[("why", &e)]), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The shortcut on its caps, or "Press keys…" while one is recorded; a
    /// click records, and its ✕ leaves none.
    fn shortcut_field(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let recording = self.recording;
        let shortcut = onyx_core::shortcut::Shortcut::from_config(&self.cfg.menu_bar.shortcut);
        let inner: AnyElement = if recording {
            div().text_color(theme.muted_foreground).child(t("Press keys…")).into_any_element()
        } else {
            match shortcut {
                Some(s) => crate::widgets::key_text(&s.to_string()).into_any_element(),
                None => div().text_color(theme.muted_foreground).child(t("None")).into_any_element(),
            }
        };
        let clear = (shortcut.is_some() && !recording).then(|| {
            let hover = theme.foreground.opacity(0.08);
            div()
                .id("bar-shortcut-clear")
                .size(px(18.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .managed_tooltip(|window, cx| tip(t("Clear"), window, cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.stop_recording(cx);
                    this.set_menu_shortcut(None, cx);
                }))
                .child(Icon::default().path("icons/close.svg").with_size(px(12.)).text_color(theme.muted_foreground))
        });
        h_flex()
            .id("bar-shortcut")
            .w(px(180.))
            .h(CONTROL_H)
            .pl(px(10.))
            .pr(px(5.))
            .gap(px(6.))
            .items_center()
            .rounded(px(8.))
            .border_1()
            .border_color(if recording { theme.primary } else { theme.foreground.opacity(0.16) })
            .when(recording, |d| d.bg(theme.primary.opacity(0.06)))
            .text_size(px(12.5))
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground.opacity(0.28)))
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                if this.recording {
                    this.stop_recording(cx);
                } else {
                    this.start_recording(cx);
                }
            }))
            .child(div().flex_1().min_w_0().child(inner))
            .children(clear)
            .into_any_element()
    }

    /// The keys pressed in Onyx's windows go to the recorder alone, and
    /// the shortcut is let go meanwhile, so pressing it again records it.
    pub(crate) fn start_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = true;
        self.record_said = None;
        crate::sys::record_keys(true);
        crate::sys::set_hot_keys(&[]);
        cx.notify();
    }

    pub(crate) fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording {
            self.recording = false;
            crate::sys::record_keys(false);
            self.hold_shortcut();
        }
        cx.notify();
    }

    /// The shortcut held again; one the system would not take is said.
    fn hold_shortcut(&mut self) {
        if !crate::hold_menu_bar_shortcut(&self.cfg.menu_bar) {
            self.record_said = Some(t("Another app holds this shortcut.").into());
        }
    }

    /// A key pressed while the shortcut is recorded: Esc alone gives up,
    /// Delete alone leaves none, and anything else is the shortcut, or the
    /// row says why it cannot be.
    pub(crate) fn recorded(&mut self, flags: u64, key: u16, cx: &mut Context<Self>) {
        use onyx_core::shortcut::{Shortcut, BACKSPACE, ESCAPE, FORWARD_DELETE, MODS};
        if !self.recording {
            return;
        }
        let alone = flags & MODS == 0;
        if key == ESCAPE && alone {
            self.stop_recording(cx);
            return;
        }
        if (key == BACKSPACE || key == FORWARD_DELETE) && alone {
            self.stop_recording(cx);
            self.set_menu_shortcut(None, cx);
            return;
        }
        match Shortcut::new(flags, key) {
            Ok(s) => {
                self.recording = false;
                crate::sys::record_keys(false);
                self.set_menu_shortcut(Some(s), cx);
            }
            Err(e) => {
                self.record_said = Some(t(e.said()).into());
                cx.notify();
            }
        }
    }

    fn set_menu_shortcut(&mut self, shortcut: Option<onyx_core::shortcut::Shortcut>, cx: &mut Context<Self>) {
        let text = shortcut.map(|s| s.to_config()).unwrap_or_default();
        self.cfg.menu_bar.shortcut = text.clone();
        let _ = Config::edit(|c| c.menu_bar.shortcut = text);
        self.record_said = None;
        self.hold_shortcut();
        cx.notify();
    }

    /// A change to the terminal's settings: kept, and the terminal drawn
    /// with it at once.
    fn set_terminal(&mut self, change: impl Fn(&mut onyx_core::config::TerminalConfig), cx: &mut Context<Self>) {
        change(&mut self.cfg.terminal);
        let _ = Config::edit(|c| change(&mut c.terminal));
        self.push_look(cx);
        cx.notify();
    }

    fn settings_pick(&mut self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        if what == "writing-language" {
            let code = id.to_string();
            self.cfg.editor.writing_language = code.clone();
            let _ = Config::edit(|c| c.editor.writing_language = code);
            self.push_writing(cx);
            self.menus.close(cx);
            cx.notify();
            return;
        }
        if what == crate::shell::SETTINGS_EDIT {
            self.menus.close(cx);
            if let Some(focus) = self.menu_focus.take() {
                crate::menu::do_edit(&id, &focus, window, cx);
            }
            return;
        }
        if what == "language" {
            let code = id.to_string();
            self.cfg.app.language = code.clone();
            let _ = Config::edit(|c| c.app.language = code.clone());
            // On a Mac the app's own `AppleLanguages` is the setting, which
            // System Settings writes too.
            crate::sys::set_own_language(if code == "system" { None } else { crate::i18n::LANGS.iter().find(|(c, _, _)| *c == code).map(|(_, _, bundle)| *bundle).or(Some("en")) });
            crate::i18n::set(&code);
            cx.set_menus(crate::app_menus());
            crate::menu_bar_lines();
            self.lang_serial += 1;
            let _ = window;
        }
        self.menus.close(cx);
        cx.refresh_windows();
        cx.notify();
    }

    fn save_section(&self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut ui = crate::ui_state::UiState::load();
        ui.settings_section = Some(self.settings_section.to_string());
        ui.save_later();
        let _ = cx;
    }
}

/// The window's top and the rail's empty room drag the window.
fn drag_region(el: Div) -> Div {
    el.on_mouse_down(MouseButton::Left, |ev, window, _| {
        if ev.click_count == 2 {
            window.titlebar_double_click();
        }
    })
    .on_mouse_move(|ev, window, _| {
        if ev.pressed_button == Some(MouseButton::Left) {
            window.start_window_move();
        }
    })
}

/// A size as words: whole when it is, else with its tenth.
fn size_words(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i32)
    } else {
        format!("{v:.1}")
    }
}
