//! The window's look: which appearance to draw (the system's, or light or
//! dark pinned), which accent, and code's inks.
//!
//! The palette stays in `themes/onyx.json`, one config per appearance.
//! An accent is a substitution over the keys that carry it there (the
//! primary button, the focus ring, links, the selection), painted into
//! both configs before they are handed to the toolkit, so `Theme::change`
//! keeps the chosen accent through every appearance switch with nothing
//! to patch afterwards.

use gpui::{App, Window};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode, ThemeSet};

use crate::i18n::phrase;

/// One accent: its key in the settings, its name (drawn through `t`),
/// and its base, hover and active shades in each appearance, plus the
/// link's shade in each.
pub struct Accent {
    pub key: &'static str,
    pub name: &'static str,
    pub light: [&'static str; 3],
    pub dark: [&'static str; 3],
    pub link: [&'static str; 2],
}

/// Seven stones, teal first and the default: onyx's sheen. The rest go
/// round the wheel from it in steps of 40 to 80 degrees, cool to warm and
/// back, so the row reads as one band: sapphire, amethyst, rose quartz,
/// carnelian, amber and jade. Each is one lightness in OKLCH, 0.52 in the
/// light appearance (amber's 0.555, where an orange at 0.52 is brown) and
/// 0.74 in the dark, at a chroma near teal's, 0.095 to 0.12 and 0.085 to
/// 0.095, so none is louder than the default: teal is the most the gamut
/// gives its hue at that lightness. Hover is 0.021 darker in the light and
/// 0.039 lighter in the dark; pressed is 0.059 and 0.041 darker. White on
/// every light base and the ink on every dark base are 4.8 to 1 or more.
pub const ACCENTS: &[Accent] = &[
    Accent { key: "teal", name: phrase("Teal"), light: ["#277580", "#1F6F79", "#0E646E"], dark: ["#6FB9C4", "#7BC5D0", "#61ACB7"], link: ["#1F6F79", "#6FB9C4"] },
    Accent { key: "sapphire", name: phrase("Sapphire"), light: ["#376AAA", "#3164A3", "#265997"], dark: ["#83AEE4", "#8FBAF1", "#77A1D7"], link: ["#3164A3", "#83AEE4"] },
    Accent { key: "amethyst", name: phrase("Amethyst"), light: ["#73599E", "#6D5398", "#62488C"], dark: ["#B39EDC", "#C0AAE9", "#A791CE"], link: ["#6D5398", "#B39EDC"] },
    Accent { key: "rose", name: phrase("Rose"), light: ["#984C70", "#914669", "#853B5F"], dark: ["#D794B1", "#E4A0BD", "#C987A4"], link: ["#914669", "#D794B1"] },
    Accent { key: "carnelian", name: phrase("Carnelian"), light: ["#A24B3B", "#9B4435", "#8F392A"], dark: ["#DF9586", "#ECA192", "#D1897A"], link: ["#9B4435", "#DF9586"] },
    Accent { key: "amber", name: phrase("Amber"), light: ["#9A6718", "#94610D", "#875601"], dark: ["#D0A165", "#DDAD72", "#C39459"], link: ["#94610D", "#D0A165"] },
    Accent { key: "jade", name: phrase("Jade"), light: ["#3C784B", "#367245", "#2A673B"], dark: ["#84BA8F", "#90C79A", "#77AD82"], link: ["#367245", "#84BA8F"] },
];

/// What is drawn on an accent: white on the light appearance's shade,
/// the ink on the dark one's, each 4.8 to 1 or more on every accent.
const ON_LIGHT: &str = "#FFFFFF";
const ON_DARK: &str = "#1F1E1D";

/// The accent by its key; an unknown key is the first.
pub fn accent(key: &str) -> &'static Accent {
    // The accents of before the stones, by the stone nearest each.
    let key = match key {
        "periwinkle" => "sapphire",
        "orchid" => "amethyst",
        "moss" => "jade",
        k => k,
    };
    ACCENTS.iter().find(|a| a.key == key).unwrap_or(&ACCENTS[0])
}

/// The accent's base in the appearance showing, for a swatch.
pub fn accent_hex(key: &str, dark: bool) -> &'static str {
    let a = accent(key);
    if dark { a.dark[0] } else { a.light[0] }
}

/// The accent as the frame wears it: the dark appearance's shade in both,
/// since the light one is 2 to 1 on the frame's graphite.
pub fn on_frame(key: &str) -> (&'static str, &'static str) {
    (accent(key).dark[0], ON_DARK)
}

/// The inks code is written in, one set an appearance: everything that
/// colors code or a terminal takes them from here, by the names the theme
/// file gives them ("@l-blue", "@d-rose"). The hues are Rosé Pine's (pine,
/// iris, gold, rose, foam, love) with a sage green beside them, each
/// taken darker or lighter until it reads at 4.5 to 1 or more on the
/// window's page, the gray for comments a step quieter.
pub struct Inks {
    pub blue: u32,
    pub violet: u32,
    pub gold: u32,
    pub rose: u32,
    pub teal: u32,
    pub green: u32,
    pub red: u32,
    pub gray: u32,
}

pub const INKS_LIGHT: Inks = Inks { blue: 0x286983, violet: 0x7A6398, gold: 0x8A6A1E, rose: 0xA3566F, teal: 0x357783, green: 0x4C7B58, red: 0xB0505F, gray: 0x85818F };
pub const INKS_DARK: Inks = Inks { blue: 0x7FB4D0, violet: 0xC4A7E7, gold: 0xE9C083, rose: 0xE8B0AE, teal: 0x9CCFD8, green: 0xA3CCA4, red: 0xEB8FA6, gray: 0x8F8BA0 };

/// The theme file with each ink's name replaced by its color.
fn inked(json: &str) -> String {
    let mut out = json.to_string();
    for (p, inks) in [("l", &INKS_LIGHT), ("d", &INKS_DARK)] {
        for (name, c) in [("blue", inks.blue), ("violet", inks.violet), ("gold", inks.gold), ("rose", inks.rose), ("teal", inks.teal), ("green", inks.green), ("red", inks.red), ("gray", inks.gray)] {
            out = out.replace(&format!("\"@{p}-{name}\""), &format!("\"#{c:06X}\""));
        }
    }
    out
}

fn paint(cfg: &mut ThemeConfig, a: &Accent) {
    let dark = cfg.mode.is_dark();
    let [base, hover, active] = if dark { a.dark } else { a.light };
    let link = if dark { a.link[1] } else { a.link[0] };
    let on = if dark { ON_DARK } else { ON_LIGHT };
    let c = &mut cfg.colors;
    c.primary = Some(base.into());
    c.primary_foreground = Some(on.into());
    c.sidebar_primary_foreground = Some(on.into());
    c.primary_hover = Some(hover.into());
    c.primary_active = Some(active.into());
    c.ring = Some(base.into());
    c.link = Some(link.into());
    c.link_hover = Some(active.into());
    c.link_active = Some(active.into());
    c.sidebar_primary = Some(base.into());
    c.list_active_border = Some(base.into());
    c.drag_border = Some(base.into());
    c.progress_bar = Some(base.into());
    c.slider_bar = Some(base.into());
    c.drop_target = Some(format!("{base}33").into());
    c.selection = Some(format!("{base}{}", if dark { "66" } else { "59" }).into());
}

/// Hand the toolkit both palettes with the accent painted in. Called at
/// start and again whenever the accent changes; `apply` then draws it.
pub fn install(accent_key: &str, cx: &mut App) {
    let set: ThemeSet = match serde_json::from_str(&inked(include_str!("../themes/onyx.json"))) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("onyx: the theme file is invalid, using the default look: {e}");
            return;
        }
    };
    let a = accent(accent_key);
    let theme = Theme::global_mut(cx);
    for mut cfg in set.themes {
        paint(&mut cfg, a);
        let cfg = std::rc::Rc::new(cfg);
        if cfg.mode.is_dark() {
            theme.dark_theme = cfg;
        } else {
            theme.light_theme = cfg;
        }
    }
}

/// Draw the appearance asked for: the system's for `system`, else the
/// one named. Called at start, at every change of the system's
/// appearance, and from the settings. Never call the toolkit's
/// `sync_system_appearance` elsewhere: a pinned appearance would flip
/// with the system.
pub fn apply(appearance: &str, window: Option<&mut Window>, cx: &mut App) {
    match appearance {
        "light" => Theme::change(ThemeMode::Light, window, cx),
        "dark" => Theme::change(ThemeMode::Dark, window, cx),
        _ => Theme::sync_system_appearance(window, cx),
    }
    // The toolkit draws again only the window it is handed; every window
    // wears the palette.
    cx.refresh_windows();
}
