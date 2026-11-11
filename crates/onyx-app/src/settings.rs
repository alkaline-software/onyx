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
use crate::widgets::{choice_pill, page_in, small_pill, wordmark, Press};

/// The window's size as it opens, and the least it is dragged to. It is
/// never narrower than it opens: the rows are laid out for that width,
/// and narrower, names broke mid-word. Shorter, the page scrolls.
pub const SETTINGS_W: Pixels = px(820.);
pub const SETTINGS_H: Pixels = px(600.);
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
    ("appearance", phrase("Appearance"), "icons/palette.svg"),
    ("text", phrase("Text"), "icons/a-large-small.svg"),
    ("terminal", phrase("Terminal"), "icons/terminal.svg"),
    ("about", phrase("About"), "icons/info.svg"),
];

/// A section by its key, the first when there is none of that name.
pub fn section_named(key: &str) -> &'static str {
    SECTIONS.iter().find(|(k, _, _)| *k == key).map_or(SECTIONS[0].0, |(k, _, _)| k)
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
                    s.make_font_inputs(window, cx);
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
            "text" if self.settings_page == "size" => self.size_rows(cx),
            "text" if self.settings_page == "spelling" => self.spelling_rows(cx),
            "text" => self.fonts_rows(cx),
            "about" => self.about_rows(cx),
            "terminal" => self.terminal_rows(cx),
            _ => self.appearance_rows(window, cx),
        };
        let title = SECTIONS.iter().find(|(k, _, _)| *k == section).map(|(_, name, _)| t(name)).unwrap_or(t("Appearance"));
        // At the title's right: Appearance has the language, Text its two
        // pages, Fonts and Size.
        let language = match section {
            "appearance" => Some(self.language_pill(cx)),
            "text" => Some(self.motion.segmented(
                "text-page",
                vec![("fonts", "", t("Fonts").into()), ("size", "", t("Size").into()), ("spelling", "", t("Spelling").into())],
                self.settings_page,
                false,
                Rc::new(|this: &mut Self, key, _, cx| {
                    this.settings_page = key;
                    cx.notify();
                }),
                cx,
            )),
            _ => None,
        };
        let pick = Rc::new(|this: &mut Self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>| this.settings_pick(what, id, window, cx));
        let close = Rc::new(|this: &mut Self, cx: &mut Context<Self>| {
            this.menus.close(cx);
            cx.notify();
        });
        let ours = |m: &crate::menu::Menu| m.what == "language" || m.what == "writing-language" || m.what == crate::shell::SETTINGS_EDIT;
        let menu = if self.menus.open.as_ref().is_some_and(ours) || self.menus.gone.as_ref().is_some_and(|(m, _)| ours(m)) { self.menus.render(pick, close, cx) } else { None };
        div()
            .id("settings-window")
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
                if this.menus.open.is_some() {
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

    fn appearance_rows(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let looks = div().w(px(CONTROL_W.into())).child(self.motion.segmented(
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
        ));
        let dark = theme.mode.is_dark();
        let chosen = crate::look::accent(&self.cfg.app.accent).key;
        let ground = card_ground(&theme);
        let swatches = h_flex().gap(px(10.)).children(crate::look::ACCENTS.iter().map(|a| {
            let key = a.key;
            let color = Rgba::try_from(crate::look::accent_hex(key, dark)).map(Hsla::from).unwrap_or(theme.primary);
            div()
                .id(SharedString::from(format!("accent-{key}")))
                .size(px(22.))
                .rounded_full()
                .bg(color)
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                // The chosen one is marked by a ring inside it, so every
                // disc is one size and the row's ends are its first and last.
                .when(key == chosen, |d| d.child(div().size(px(14.)).rounded_full().border_2().border_color(ground)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.cfg.app.accent = key.to_string();
                    let _ = Config::edit(|c| c.app.accent = key.to_string());
                    crate::look::install(key, cx);
                    let appearance = this.cfg.app.appearance.clone();
                    crate::look::apply(&appearance, Some(window), cx);
                    cx.notify();
                }))
        }));
        let accent_name = t(crate::look::accent(&self.cfg.app.accent).name);
        // The person: their picture, a press on it or on Choose… picks
        // one, and their name in a field beside it.
        let name_field = self.name_input.as_ref().map(|input| {
            let focus = input.read(cx).focus_handle(cx);
            div().id("name-field").track_focus(&focus).w(px(150.)).h(CONTROL_H).px(px(8.)).flex().items_center().rounded(px(6.)).bg(if dark { theme.muted } else { theme.background }).border_1().border_color(theme.input).text_size(px(12.5)).child(Input::new(input).appearance(false).on_secondary_click(Self::input_menu(input, crate::shell::SETTINGS_EDIT, cx)).bordered(false))
        });
        let you = h_flex()
            .gap(px(8.))
            .items_center()
            .child(self.avatar(28., &theme).id("settings-avatar").cursor_pointer().hover(|s| s.opacity(0.85)).on_click(cx.listener(|this, _, _, cx| this.pick_avatar(cx))))
            .child(small_pill("avatar-choose", "icons/image.svg", t("Choose…"), Press::Quiet, false, &theme).h(CONTROL_H).on_click(cx.listener(|this, _, _, cx| this.pick_avatar(cx))))
            .when(!self.cfg.app.avatar.is_empty(), |d| d.child(small_pill("avatar-remove", "icons/trash.svg", t("Remove"), Press::Danger, false, &theme).h(CONTROL_H).on_click(cx.listener(|this, _, _, cx| this.set_avatar(String::new(), cx)))))
            .children(name_field)
            .into_any_element();
        vec![
            group(None, None, vec![line(t("Appearance").into(), None, looks.into_any_element(), &theme), line(t("Accent").into(), Some(accent_name.into()), swatches.into_any_element(), &theme)], &theme),
            group(None, None, vec![line(t("Your picture and name").into(), Some(t("How the vault's page greets you and the sidebar shows you.").into()), you, &theme)], &theme),
        ]
    }

    /// Editor: the faces a note is set in, each a tile drawn in itself;
    /// the words' size; and how wide a line runs.
    /// Text, Fonts: the note's face, serifs over sans serifs, and code's
    /// face, each on a tile drawn in itself, as emaki has them.
    fn fonts_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        // A font of the machine's named in a field stands over the tiles,
        // which then light none.
        let e = &self.cfg.editor;
        let text_now = if e.text_custom.trim().is_empty() { e.text_face.clone() } else { String::new() };
        let mono_now = if e.mono_custom.trim().is_empty() { e.mono_face.clone() } else { String::new() };
        let cjk_now = if e.cjk_custom.trim().is_empty() { e.cjk_face.clone() } else { String::new() };
        let serif = crate::fonts::is_serif(&self.look.text_family);
        let tile_ground = if theme.mode.is_dark() { theme.muted } else { theme.popover };
        let tiles = |faces: &'static [crate::fonts::Face], now: &str, sample: &'static str, kind: u8, cx: &mut Context<Self>| {
            h_flex().gap(px(8.)).children(faces.iter().map(|face| {
                let (key, active) = (face.key, face.key == now);
                let mono = kind == 1;
                // A Chinese tile draws its sample in its own face; Auto in
                // the one the note's face would get.
                let family: SharedString = if kind == 2 { crate::fonts::cjk_family(face.key, serif) } else { face.family.into() };
                let name: SharedString = if face.key == "auto" { t("Auto").into() } else { face.name.into() };
                v_flex()
                    .id(SharedString::from(format!("face-{key}")))
                    .flex_1()
                    .min_w_0()
                    .h(px(64.))
                    .px(px(12.))
                    .justify_center()
                    .gap(px(2.))
                    .rounded(px(10.))
                    .border_1()
                    .cursor_pointer()
                    .bg(tile_ground)
                    .border_color(if active { theme.primary } else { theme.border })
                    .when(active, |d| d.bg(theme.primary.opacity(0.08)))
                    .when(!active, |d| d.hover(|s| s.border_color(theme.muted_foreground)))
                    .on_click(cx.listener(move |this, _, window, cx| match kind {
                        1 => this.set_mono_face(key, window, cx),
                        2 => this.set_cjk_face(key, window, cx),
                        _ => this.set_text_face(key, window, cx),
                    }))
                    // One line, at a size that fits the narrowest tile.
                    .child(div().whitespace_nowrap().overflow_hidden().text_size(px(if mono { 14. } else { 20. })).line_height(px(25.)).font_family(family).child(sample))
                    .child(div().truncate().text_size(px(11.5)).text_color(if active { theme.foreground } else { theme.muted_foreground }).child(name))
            }))
        };
        // A kind of face, small and quiet, over its tiles.
        let kind = |name: SharedString| div().text_size(px(11.5)).text_color(theme.muted_foreground).child(name);
        let note = v_flex()
            .p(px(14.))
            .gap(px(8.))
            .child(kind(t("Serif").into()))
            .child(tiles(crate::fonts::SERIFS, &text_now, "Ag", 0, cx))
            .child(div().h(px(4.)))
            .child(kind(t("Sans serif").into()))
            .child(tiles(crate::fonts::SANS, &text_now, "Ag", 0, cx))
            .into_any_element();
        let mono = div().p(px(14.)).child(tiles(crate::fonts::MONOS, &mono_now, "fn(0) -> 1", 1, cx)).into_any_element();
        let cjk = div().p(px(14.)).child(tiles(crate::fonts::CJK, &cjk_now, "汉字", 2, cx)).into_any_element();
        // Under each card: a field for a font of the machine's, as
        // Obsidian offers one, which stands over the tiles while it has a name.
        let field = |ix: usize, cx: &mut Context<Self>| -> Option<AnyElement> {
            let input = self.font_inputs.as_ref()?[ix].clone();
            let focus = input.read(cx).focus_handle(cx);
            let plate = div().id(("font-field", ix)).track_focus(&focus).w(px(220.)).h(CONTROL_H).px(px(8.)).flex().items_center().rounded(px(8.)).bg(theme.muted).text_size(px(13.)).child(Input::new(&input).appearance(false).on_secondary_click(Self::input_menu(&input, crate::shell::SETTINGS_EDIT, cx))).into_any_element();
            Some(line(t("Installed font").into(), Some(t("A font on this Mac, by its name in Font Book.").into()), plate, &theme))
        };
        let mut rows = Vec::new();
        let titles: [SharedString; 3] = [t("Note font").into(), t("Monospace font").into(), t("Chinese font").into()];
        for (title, body, ix) in [(titles[0].clone(), note, 0), (titles[1].clone(), mono, 1), (titles[2].clone(), cjk, 2)] {
            let mut card = vec![body];
            card.extend(field(ix, cx));
            rows.push(group(Some(title), None, card, &theme));
        }
        rows
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

    /// The three installed-font fields, made with the settings window and
    /// holding what the settings say; a change is kept as typed.
    pub(crate) fn make_font_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values = [self.cfg.editor.text_custom.clone(), self.cfg.editor.mono_custom.clone(), self.cfg.editor.cjk_custom.clone()];
        let inputs: [Entity<InputState>; 3] = std::array::from_fn(|ix| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(t("Font name")));
            let value = values[ix].clone();
            input.update(cx, |i, cx| i.set_value(value, window, cx));
            cx.subscribe_in(&input, window, move |this, input, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    let text = input.read(cx).value().to_string();
                    this.set_custom_font(ix, text, cx);
                }
            })
            .detach();
            input
        });
        self.font_inputs = Some(inputs);
    }

    /// A font of the machine's named, or the name cleared.
    fn set_custom_font(&mut self, ix: usize, name: String, cx: &mut Context<Self>) {
        let e = &mut self.cfg.editor;
        let slot = match ix {
            1 => &mut e.mono_custom,
            2 => &mut e.cjk_custom,
            _ => &mut e.text_custom,
        };
        if *slot == name {
            return;
        }
        *slot = name;
        let e = self.cfg.editor.clone();
        let _ = Config::edit(|c| c.editor = e);
        self.push_look(cx);
        cx.notify();
    }

    /// A tile chosen clears the field over it, so the tile takes effect.
    fn clear_custom(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(inputs) = self.font_inputs.as_ref() {
            let input = inputs[ix].clone();
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        match ix {
            1 => self.cfg.editor.mono_custom.clear(),
            2 => self.cfg.editor.cjk_custom.clear(),
            _ => self.cfg.editor.text_custom.clear(),
        }
    }

    /// Text, Size: a preset for both places at once, and each place's
    /// size under it, which a preset sets. Custom is the person's own,
    /// set here by a step, and kept while a preset is chosen; under a
    /// preset the sizes are its and are only shown. Then the layout.
    fn size_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let sizes = self.cfg.editor.text_sizes();
        let now = match self.cfg.editor.text_size.as_str() {
            "small" => "small",
            "large" => "large",
            "custom" => "custom",
            _ => "medium",
        };
        let presets = div()
            .w(CONTROL_W)
            .child(self.motion.segmented(
                "text-size",
                vec![("small", "", t("Small").into()), ("medium", "", t("Medium").into()), ("large", "", t("Large").into()), ("custom", "", t("Custom").into())],
                now,
                true,
                Rc::new(|this: &mut Self, key, _, cx| this.set_text_size(key, cx)),
                cx,
            ))
            .into_any_element();
        let custom = now == "custom";
        let places: [(&'static str, SharedString, f32); 2] = [("note", t("Note text").into(), sizes.note), ("code", t("Code").into(), sizes.code)];
        let rows = places
            .into_iter()
            .map(|(key, name, value)| {
                let control = h_flex().gap(px(8.)).items_center().children(self.size_reset(key, value, custom, cx)).child(self.stepper(key, value, custom, cx)).into_any_element();
                line(name, None, control, &theme)
            })
            .collect();
        vec![group(None, None, vec![line(t("Size").into(), None, presets, &theme)], &theme), group(None, None, rows, &theme)]
    }

    /// Under Custom, beside a size not Medium's: a button that puts it
    /// back to Medium's, which is Custom's own until it is changed. It
    /// comes and goes in motion (`presence`).
    fn size_reset(&self, key: &'static str, value: f32, custom: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        use onyx_core::config::TextSizes;
        let medium = size_of(&TextSizes::MEDIUM, key);
        let shown = custom && value != medium;
        let id = format!("size-reset-{key}");
        let moving = self.motion.presence(&id, shown, cx);
        if !shown && moving.is_none() {
            return None;
        }
        let theme = cx.theme().clone();
        let hover = theme.foreground.opacity(0.07);
        let tip: SharedString = tf("Reset to {size} pt", &[("size", &size_words(medium))]).into();
        let button = div()
            .id(SharedString::from(id.clone()))
            .size(px(26.))
            .flex_shrink_0()
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .when(shown, |d| {
                d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |this, _, window, cx| {
                    crate::widgets::swallow_click(window, cx);
                    this.set_sizes(|s| *size_field(s, key) = medium, cx);
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
        self.stepper_of(key, value, live, down, up, Rc::new(move |this: &mut Self, to, cx| this.set_sizes(|s| *size_field(s, key) = to, cx)), cx)
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

    fn set_text_face(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_custom(0, window, cx);
        self.cfg.editor.text_face = key.to_string();
        self.save_editor(cx);
    }

    fn set_mono_face(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_custom(1, window, cx);
        self.cfg.editor.mono_face = key.to_string();
        self.save_editor(cx);
    }

    fn set_cjk_face(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_custom(2, window, cx);
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

    /// A preset chosen, or Custom: Custom begins as Medium's sizes the
    /// first time, and keeps its own after.
    fn set_text_size(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if key == "custom" && self.cfg.editor.sizes.is_none() {
            self.cfg.editor.sizes = Some(onyx_core::config::TextSizes::MEDIUM);
        }
        self.cfg.editor.text_size = key.to_string();
        let e = self.cfg.editor.clone();
        let _ = Config::edit(|c| c.editor = e);
        self.push_look(cx);
        cx.notify();
    }

    /// One place's own size changed, under Custom, where they are set.
    fn set_sizes(&mut self, change: impl FnOnce(&mut onyx_core::config::TextSizes), cx: &mut Context<Self>) {
        if self.cfg.editor.text_size != "custom" {
            return;
        }
        let mut sizes = self.cfg.editor.text_sizes();
        change(&mut sizes);
        self.cfg.editor.sizes = Some(sizes.clamped());
        let e = self.cfg.editor.clone();
        let _ = Config::edit(|c| c.editor = e);
        self.push_look(cx);
        cx.notify();
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

    /// The window's language, at the right of Appearance's name: the
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

    /// Spelling and grammar: the check on or off, and what it reads in.
    /// English is read here, for grammar too; the others are the system's
    /// spelling (`docs/editor.md`).
    fn spelling_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
        let language = div().when(!on, |d| d.opacity(0.45).occlude()).child(pill).into_any_element();
        vec![group(None, None, vec![line(t("Check spelling and grammar").into(), None, check, &theme), line(t("Language").into(), None, language, &theme)], &theme)]
    }

    /// The terminal's look and behavior, as Emaki's Settings, Terminal
    /// has them: its scheme, its cursor, and the face, which is the
    /// window's code face, set in Text, Fonts.
    fn terminal_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let term = self.cfg.terminal.clone();
        let pick = |keys: &[&'static str], now: &str| keys.iter().copied().find(|k| *k == now).unwrap_or(keys[0]);
        let scheme = div().w(CONTROL_W).child(self.motion.segmented("term-theme", vec![("match", "", t("Match window").into()), ("dark", "", t("Dark").into()), ("light", "", t("Light").into())], pick(&["match", "dark", "light"], &term.theme), true, Rc::new(|this: &mut Self, key, _, cx| this.set_terminal(|t| t.theme = key.into(), cx)), cx)).into_any_element();
        let cursor = div().w(CONTROL_W).child(self.motion.segmented("term-cursor", vec![("bar", "", t("Bar").into()), ("block", "", t("Block").into()), ("underline", "", t("Underline").into())], pick(&["bar", "block", "underline"], &term.cursor), true, Rc::new(|this: &mut Self, key, _, cx| this.set_terminal(|t| t.cursor = key.into(), cx)), cx)).into_any_element();
        let blink = self.motion.switch("term-blink", term.cursor_blink, Rc::new(|this: &mut Self, on, _, cx| this.set_terminal(|t| t.cursor_blink = on, cx)), cx);
        let ligatures = self.motion.switch("term-ligatures", term.ligatures, Rc::new(|this: &mut Self, on, _, cx| this.set_terminal(|t| t.ligatures = on, cx)), cx);
        let copy = self.motion.switch("term-copy", term.copy_on_select, Rc::new(|this: &mut Self, on, _, cx| this.set_terminal(|t| t.copy_on_select = on, cx)), cx);
        // The face is the window's code face, set in Text, Fonts.
        let fonts = choice_pill("term-fonts", "icons/a-large-small.svg", t("Open Fonts"), Press::Quiet, false, &theme)
            .h(CONTROL_H)
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_section = "text";
                this.settings_page = "fonts";
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

/// A place's size in the sizes, by the row's key.
fn size_field<'a>(s: &'a mut onyx_core::config::TextSizes, key: &str) -> &'a mut f32 {
    match key {
        "code" => &mut s.code,
        _ => &mut s.note,
    }
}

fn size_of(s: &onyx_core::config::TextSizes, key: &str) -> f32 {
    match key {
        "code" => s.code,
        _ => s.note,
    }
}

/// A size as words: whole when it is, else with its tenth.
fn size_words(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i32)
    } else {
        format!("{v:.1}")
    }
}
