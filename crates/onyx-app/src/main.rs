// A release build on Windows has no console window beside it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ambient;
mod assets;
mod csd;
mod editor;
mod export;
#[allow(dead_code)]
mod file_icons;
mod fonts;
mod i18n;
mod langs;
mod layout;
mod lines;
mod look;
mod markdown;
mod menu;
mod scroll;
mod settings;
mod shell;
mod tabs;
mod sys;
mod tables;
mod ui_state;
mod widgets;

use gpui::*;
use gpui_component::Root;

use i18n::t;
use onyx_core::config::Config;
use shell::{CloseTab, CloseVault, Escape, ExportPdf, PrintNote, FindInNote, FindNext, FindPrev, NewFolder, NewNote, NewVault, ReopenTab, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, Tab9, OpenSettings, OpenVault, Save, Shell, ShowLive, ShowNarrow, ShowMiddle, ShowReading, ShowSource, ShowWide, ToggleReading, ToggleSearch, ToggleSidebar, KEY_CONTEXT, MIN_W};

actions!(onyx_app, [Quit, CloseWindow, Hide, HideOthers, ShowAll, Minimize, Zoom, ToggleFullScreen]);

/// A shortcut is named as the Mac has it and bound as `secondary-<key>`,
/// which gpui makes ⌘ on a Mac and Ctrl elsewhere. The editor's own keys
/// are in `editor::key_bindings`.
fn key_bindings() -> Vec<KeyBinding> {
    let mut keys = vec![
        KeyBinding::new("secondary-shift-b", ToggleSidebar, None),
        KeyBinding::new("secondary-n", NewNote, None),
        KeyBinding::new("secondary-shift-n", NewFolder, None),
        KeyBinding::new("secondary-shift-o", OpenVault, None),
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-shift-e", ExportPdf, None),
        KeyBinding::new("secondary-p", PrintNote, None),
        KeyBinding::new("enter", export::ExportNow, Some(shell::EXPORT_CONTEXT)),
        KeyBinding::new("left", export::PagePrev, Some(shell::EXPORT_CONTEXT)),
        KeyBinding::new("right", export::PageNext, Some(shell::EXPORT_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(shell::EXPORT_CONTEXT)),
        KeyBinding::new("secondary-e", ToggleReading, None),
        KeyBinding::new("secondary-k", ToggleSearch, None),
        KeyBinding::new("secondary-f", FindInNote, None),
        KeyBinding::new("secondary-g", FindNext, None),
        KeyBinding::new("secondary-shift-g", FindPrev, None),
        KeyBinding::new("escape", Escape, Some(shell::SEARCH_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(shell::FIND_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(shell::TITLE_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(settings::SETTINGS_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(editor::EDITOR_CONTEXT)),
        KeyBinding::new("escape", Escape, Some("Sheet")),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("secondary-shift-w", CloseWindow, None),
        KeyBinding::new("secondary-shift-t", ReopenTab, None),
        KeyBinding::new("secondary-1", Tab1, None),
        KeyBinding::new("secondary-2", Tab2, None),
        KeyBinding::new("secondary-3", Tab3, None),
        KeyBinding::new("secondary-4", Tab4, None),
        KeyBinding::new("secondary-5", Tab5, None),
        KeyBinding::new("secondary-6", Tab6, None),
        KeyBinding::new("secondary-7", Tab7, None),
        KeyBinding::new("secondary-8", Tab8, None),
        KeyBinding::new("secondary-9", Tab9, None),
        // ⌃ with a digit shows a way of reading on a Mac. Elsewhere ctrl
        // is `secondary`, which picks a tab, so the modes add alt.
        KeyBinding::new(if cfg!(target_os = "macos") { "ctrl-1" } else { "ctrl-alt-1" }, ShowLive, None),
        KeyBinding::new(if cfg!(target_os = "macos") { "ctrl-2" } else { "ctrl-alt-2" }, ShowSource, None),
        KeyBinding::new(if cfg!(target_os = "macos") { "ctrl-3" } else { "ctrl-alt-3" }, ShowReading, None),
        KeyBinding::new("alt-1", ShowNarrow, None),
        KeyBinding::new("alt-2", ShowMiddle, None),
        KeyBinding::new("alt-3", ShowWide, None),
        KeyBinding::new("secondary-q", Quit, None),
    ];
    keys.extend(editor::key_bindings());
    if cfg!(target_os = "macos") {
        keys.extend([KeyBinding::new("secondary-h", Hide, None), KeyBinding::new("secondary-alt-h", HideOthers, None), KeyBinding::new("secondary-m", Minimize, None), KeyBinding::new("ctrl-secondary-f", ToggleFullScreen, None)]);
    }
    keys
}

/// The menus, made again when the window's language changes.
pub(crate) fn app_menus() -> Vec<Menu> {
    let mac = cfg!(target_os = "macos");
    let mut app_items = vec![MenuItem::action(t("Settings…"), OpenSettings), MenuItem::separator()];
    if mac {
        app_items.extend([MenuItem::action(t("Hide Onyx"), Hide), MenuItem::action(t("Hide Others"), HideOthers), MenuItem::action(t("Show All"), ShowAll), MenuItem::separator()]);
    }
    app_items.push(MenuItem::action(t("Quit Onyx"), Quit));
    let mut menus = vec![
        Menu { name: "Onyx".into(), items: app_items, disabled: false },
        Menu {
            name: t("File").into(),
            disabled: false,
            items: vec![
                MenuItem::action(t("New Note"), NewNote),
                MenuItem::action(t("New Folder"), NewFolder),
                MenuItem::separator(),
                MenuItem::action(t("New Vault…"), NewVault),
                MenuItem::action(t("Open Vault…"), OpenVault),
                MenuItem::action(t("Close Vault"), CloseVault),
                MenuItem::separator(),
                MenuItem::action(t("Save"), Save),
                MenuItem::action(t("Export to PDF…"), ExportPdf),
                MenuItem::action(t("Print…"), PrintNote),
                MenuItem::separator(),
                MenuItem::action(t("Close Tab"), CloseTab),
                MenuItem::action(t("Reopen Closed Tab"), ReopenTab),
                MenuItem::action(t("Close Window"), CloseWindow),
            ],
        },
    ];
    if mac {
        menus.push(Menu {
            name: t("Edit").into(),
            disabled: false,
            items: vec![
                MenuItem::os_action(t("Undo"), gpui_component::input::Undo, OsAction::Undo),
                MenuItem::os_action(t("Redo"), gpui_component::input::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action(t("Cut"), gpui_component::input::Cut, OsAction::Cut),
                MenuItem::os_action(t("Copy"), gpui_component::input::Copy, OsAction::Copy),
                MenuItem::os_action(t("Paste"), gpui_component::input::Paste, OsAction::Paste),
                MenuItem::os_action(t("Select All"), gpui_component::input::SelectAll, OsAction::SelectAll),
                MenuItem::separator(),
                MenuItem::action(t("Bold"), editor::Bold),
                MenuItem::action(t("Italic"), editor::Italic),
                MenuItem::action(t("Toggle Task"), editor::ToggleTask),
                MenuItem::separator(),
                MenuItem::action(t("Find…"), FindInNote),
                MenuItem::action(t("Find Next"), FindNext),
                MenuItem::action(t("Find Previous"), FindPrev),
            ],
        });
    }
    menus.push(Menu {
        name: t("View").into(),
        disabled: false,
        items: vec![MenuItem::action(t("Edit"), ShowLive), MenuItem::action(t("Source"), ShowSource), MenuItem::action(t("Read"), ShowReading), MenuItem::separator(), MenuItem::action(t("Narrow"), ShowNarrow), MenuItem::action(t("Middle"), ShowMiddle), MenuItem::action(t("Wide"), ShowWide), MenuItem::separator(), MenuItem::action(t("Search…"), ToggleSearch), MenuItem::separator(), MenuItem::action(t("Toggle Sidebar"), ToggleSidebar)],
    });
    menus.push(Menu { name: t("Window").into(), disabled: false, items: vec![MenuItem::action(t("Minimize"), Minimize), MenuItem::action(t("Zoom"), Zoom), MenuItem::action(t("Toggle Full Screen"), ToggleFullScreen)] });
    menus
}

/// The main window's view, for an action that reaches no view.
struct MainShell(WeakEntity<Shell>, AnyWindowHandle);

impl Global for MainShell {}

fn with_active_window(cx: &mut App, f: impl FnOnce(&mut Window) + 'static) {
    let Some(w) = cx.active_window() else { return };
    cx.defer(move |cx| {
        w.update(cx, |_, window, _| f(window)).ok();
    });
}

/// The remembered bounds, when their center is still on a screen, with an
/// edge within two points of the screen's usable area taken to it: a
/// window a point short of the screen showed the window behind it there.
fn remembered_bounds(cx: &App) -> Option<Bounds<Pixels>> {
    let r = ui_state::UiState::load().window?;
    let b = Bounds { origin: point(px(r.x), px(r.y)), size: size(px(r.w).max(MIN_W), px(r.h.max(480.))) };
    let center = point(b.origin.x + b.size.width / 2., b.origin.y + b.size.height / 2.);
    let display = cx.displays().into_iter().find(|d| d.bounds().contains(&center))?;
    let area = display.visible_bounds();
    let near = |a: Pixels, to: Pixels| a != to && (a - to).abs() <= px(2.);
    let (mut l, mut tp, mut r2, mut bt) = (b.left(), b.top(), b.right(), b.bottom());
    if near(l, area.left()) {
        l = area.left();
    }
    if near(tp, area.top()) {
        tp = area.top();
    }
    if near(r2, area.right()) {
        r2 = area.right();
    }
    if near(bt, area.bottom()) {
        bt = area.bottom();
    }
    Some(Bounds::from_corners(point(l, tp), point(r2, bt)))
}

fn open_main_window(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let bounds = remembered_bounds(cx).unwrap_or_else(|| Bounds::centered(None, size(px(1080.), px(720.)), cx));
    let titlebar = if cfg!(target_os = "macos") {
        TitlebarOptions { title: None, appears_transparent: true, traffic_light_position: Some(point(px(sys::LIGHTS_ROW.0), px(sys::LIGHTS_ROW.1))) }
    } else {
        TitlebarOptions { title: Some("Onyx".into()), appears_transparent: false, traffic_light_position: None }
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(titlebar),
            window_min_size: Some(size(MIN_W, px(480.))),
            // The desktop file's name, or a Linux shell shows a blank icon.
            app_id: Some("Onyx".into()),
            app_owns_titlebar_drag: true,
            ..Default::default()
        },
        |window, cx| {
            window
                .observe_window_appearance(|window, cx| {
                    let appearance = Config::load().app.appearance;
                    look::apply(&appearance, Some(window), cx);
                })
                .detach();
            look::apply(&Config::load().app.appearance, Some(window), cx);
            let shell = cx.new(|cx| Shell::new(window, cx));
            let focus = shell.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            // What the window remembers is written as its close button
            // closes it: on Windows and Linux that ends the app with no
            // quit to save at.
            let asked = shell.downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                let _ = asked.update(cx, |s, cx| {
                    s.save_all(cx);
                    s.save_ui_now(window)
                });
                true
            });
            cx.set_global(MainShell(shell.downgrade(), window.window_handle()));
            // For a script: steps, a section of the settings, a picture.
            let go = std::env::var("ONYX_GO").ok();
            let section = std::env::var("ONYX_SETTINGS").ok();
            let shot = std::env::var_os("ONYX_SHOT").filter(|p| !p.is_empty());
            shell.update(cx, |s, cx| {
                if let Some(steps) = go.as_deref() {
                    sys::keep_on_top(window);
                    s.go(steps, window, cx);
                }
                if let Some(section) = section.as_deref() {
                    // `size` is Text's Size page.
                    if section == "size" || section == "spelling" {
                        s.settings_page = if section == "size" { "size" } else { "spelling" };
                    }
                    s.open_settings(Some(settings::section_named(if section == "size" || section == "spelling" { "text" } else { section })), window, cx);
                }
                if let Some(path) = shot {
                    s.shoot_later(path.into(), window, cx);
                }
            });
            cx.new(|cx| Root::new(shell, window, cx))
        },
    )
}

fn main() {
    // Velopack's installer runs the app with a hook to answer before
    // anything else.
    #[cfg(windows)]
    velopack::VelopackApp::build().run();
    let app = gpui_platform::application().with_assets(assets::Assets);
    // The app stays running with no window; a Dock click opens one.
    app.on_reopen(|cx| {
        let main = cx.try_global::<MainShell>().map(|m| m.1);
        if !main.is_some_and(|h| cx.windows().contains(&h)) {
            if let Err(e) = open_main_window(cx) {
                eprintln!("onyx: could not open a window: {e}");
            }
        }
        cx.activate(true);
    });
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        gpui_component::tooltip::set_label(|text, window, cx| widgets::tip(text, window, cx));
        sys::install_dock_icon();
        sys::warm_icloud();
        let cfg = Config::load();
        i18n::set(&i18n::chosen(&cfg.app.language));
        fonts::install(cx);
        look::install(&cfg.app.accent, cx);
        look::apply(&cfg.app.appearance, None, cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &CloseWindow, cx| with_active_window(cx, |w| w.remove_window()));
        // ⌘W in a window with no tabs to close (the settings) closes it;
        // the main window answers it first, above.
        cx.on_action(|_: &CloseTab, cx| with_active_window(cx, |w| w.remove_window()));
        cx.on_action(|_: &Minimize, cx| with_active_window(cx, |w| w.minimize_window()));
        cx.on_action(|_: &Zoom, cx| with_active_window(cx, |w| w.zoom_window()));
        cx.on_action(|_: &ToggleFullScreen, cx| with_active_window(cx, |w| w.toggle_fullscreen()));
        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
        cx.on_action(|_: &OpenSettings, cx| {
            let Some(shell) = cx.try_global::<MainShell>().map(|m| m.0.clone()) else { return };
            cx.defer(move |cx| settings::open(shell, cx));
        });

        cx.bind_keys(key_bindings());
        cx.set_menus(app_menus());

        if let Err(e) = open_main_window(cx) {
            eprintln!("onyx: could not open a window: {e}");
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
