//! The faces the window carries, and which face each place uses.
//!
//! The chrome is the system's face and is never set here. A note's words
//! are set in a face of the person's choice (`SERIFS`, `SANS`), code in a
//! monospace face of theirs (`MONOS`), the same twelve emaki offers and
//! under the same keys, Chinese in a carried face of theirs behind them
//! (`CJK`), any of the three a font of the machine's instead when the
//! person names one, and the app's name (the
//! sidebar's wordmark, About's title card) in Optima where the system
//! has it, else Belleza, a free face with the same flared strokes,
//! carried for Windows and Linux. Each carried face is a static file a
//! weight under the SIL Open Font License, with that license beside the
//! files in `assets/fonts/`: handed a variable font, gpui holds one face
//! of it and draws every bold as regular.

use std::borrow::Cow;

use gpui::{App, SharedString};

/// A face a person may choose: its key in the settings, the name drawn
/// on its tile (a proper name, the same in every language), and the
/// family name the text system knows.
pub struct Face {
    pub key: &'static str,
    pub name: &'static str,
    pub family: &'static str,
}

/// The faces a note's words may be set in, as emaki offers them: four
/// serifs and four sans serifs. Inter is a new install's.
pub const SERIFS: &[Face] = &[
    Face { key: "source-serif", name: "Source Serif", family: "Source Serif 4" },
    Face { key: "newsreader", name: "Newsreader", family: "Newsreader" },
    Face { key: "literata", name: "Literata", family: "Literata" },
    Face { key: "lora", name: "Lora", family: "Lora" },
];

pub const SANS: &[Face] = &[
    Face { key: "inter", name: "Inter", family: "Inter" },
    Face { key: "plex-sans", name: "IBM Plex Sans", family: "IBM Plex Sans" },
    Face { key: "source-sans", name: "Source Sans", family: "Source Sans 3" },
    Face { key: "geist", name: "Geist", family: "Geist" },
];

/// The faces code is set in, JetBrains Mono first and a new install's.
/// The second is the system's where it has one worth having: Monaco on
/// a Mac, which is Apple's and is used from the system there, never
/// carried. Elsewhere it is IBM Plex Mono, built in.
pub const MONOS: &[Face] = &[
    Face { key: "jetbrains", name: "JetBrains Mono", family: "JetBrains Mono" },
    if cfg!(target_os = "macos") { Face { key: "monaco", name: "Monaco", family: "Monaco" } } else { Face { key: "plex-mono", name: "IBM Plex Mono", family: "IBM Plex Mono" } },
    Face { key: "maple", name: "Maple Mono", family: "Maple Mono" },
    Face { key: "geist-mono", name: "Geist Mono", family: "Geist Mono" },
];

/// The faces Chinese, Japanese and Korean may be set in, carried: Noto
/// Serif SC and Noto Sans SC (Adobe and Google's Source Han faces, the
/// simplified Chinese subset), and LXGW WenKai, a kai hand. `auto` is
/// none of them by name: Noto Serif SC behind a serif face and Noto
/// Sans SC behind a sans serif or a monospace one.
pub const CJK: &[Face] = &[
    Face { key: "auto", name: "Auto", family: "" },
    Face { key: "noto-serif-sc", name: "Noto Serif SC", family: "Noto Serif SC" },
    Face { key: "noto-sans-sc", name: "Noto Sans SC", family: "Noto Sans SC" },
    Face { key: "wenkai", name: "LXGW WenKai", family: "LXGW WenKai" },
];

const FACES: &[&[u8]] = &[
    include_bytes!("../assets/fonts/NotoSerifSC-Regular.otf"),
    include_bytes!("../assets/fonts/NotoSerifSC-Bold.otf"),
    include_bytes!("../assets/fonts/NotoSansSC-Regular.otf"),
    include_bytes!("../assets/fonts/NotoSansSC-Bold.otf"),
    include_bytes!("../assets/fonts/LXGWWenKai-Regular.ttf"),
    include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf"),
    include_bytes!("../assets/fonts/Belleza-Regular.ttf"),
    include_bytes!("../assets/fonts/Geist-Bold.ttf"),
    include_bytes!("../assets/fonts/Geist-Italic.ttf"),
    include_bytes!("../assets/fonts/Geist-Regular.ttf"),
    include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Bold.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Italic.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Italic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Italic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Literata-Bold.ttf"),
    include_bytes!("../assets/fonts/Literata-Italic.ttf"),
    include_bytes!("../assets/fonts/Literata-Regular.ttf"),
    include_bytes!("../assets/fonts/Literata-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Lora-Bold.ttf"),
    include_bytes!("../assets/fonts/Lora-Italic.ttf"),
    include_bytes!("../assets/fonts/Lora-Regular.ttf"),
    include_bytes!("../assets/fonts/Lora-SemiBold.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Bold.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Italic.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Medium.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Regular.ttf"),
    include_bytes!("../assets/fonts/MapleMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Newsreader-Bold.ttf"),
    include_bytes!("../assets/fonts/Newsreader-Italic.ttf"),
    include_bytes!("../assets/fonts/Newsreader-Regular.ttf"),
    include_bytes!("../assets/fonts/Newsreader-SemiBold.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Bold.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Italic.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Regular.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-SemiBold.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Bold.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Italic.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-SemiBold.ttf"),
];

/// The family a Chinese face is known by, for a tile: `auto` shows the
/// one a serif or a sans serif note would get.
pub fn cjk_family(key: &str, serif: bool) -> SharedString {
    match CJK.iter().find(|f| f.key == key) {
        Some(f) if !f.family.is_empty() => f.family.into(),
        _ => if serif { CJK[1].family } else { CJK[2].family }.into(),
    }
}

/// Whether a family is one of the serifs, carried or the person's own
/// by the look of its name.
pub fn is_serif(family: &str) -> bool {
    SERIFS.iter().any(|f| f.family == family) || family.to_ascii_lowercase().contains("serif") && !family.to_ascii_lowercase().contains("sans")
}

/// The faces Chinese, Japanese and Korean fall back on behind a Latin
/// face, which carries none of their characters: the person's own
/// installed one first when named, then the carried face chosen (or,
/// under `auto`, a serif's behind a serif and a sans serif's behind a
/// sans serif or a monospace face), then the system's faces of that
/// kind for Japanese, Korean and whatever else the carried face lacks,
/// and the system's emoji last. A note set in Source Serif has its
/// Chinese in a serif too and not in the system's sans.
pub fn cjk_fallbacks(family: &str, cjk_key: &str, cjk_custom: &str) -> gpui::FontFallbacks {
    let serif = is_serif(family);
    let mut faces: Vec<String> = Vec::new();
    if !cjk_custom.trim().is_empty() {
        faces.push(cjk_custom.trim().to_string());
    }
    faces.push(cjk_family(cjk_key, serif).to_string());
    faces.extend(system_fallbacks(serif).iter().map(|f| f.to_string()));
    gpui::FontFallbacks::from_fonts(faces)
}

/// The system's faces for what the carried ones lack, and its emoji last.
fn system_fallbacks(serif: bool) -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        if serif { &["Songti SC", "Songti TC", "Hiragino Mincho ProN", "Apple SD Gothic Neo", "Apple Color Emoji"] } else { &["PingFang SC", "PingFang TC", "Hiragino Sans", "Apple SD Gothic Neo", "Apple Color Emoji"] }
    } else if cfg!(target_os = "windows") {
        if serif { &["SimSun", "PMingLiU", "Yu Mincho", "Batang", "Segoe UI Emoji"] } else { &["Microsoft YaHei", "Microsoft JhengHei", "Yu Gothic", "Malgun Gothic", "Segoe UI Emoji"] }
    } else if serif {
        &["Noto Serif CJK SC", "Noto Serif CJK TC", "Noto Serif CJK JP", "Noto Serif CJK KR", "Noto Color Emoji"]
    } else {
        &["Noto Sans CJK SC", "Noto Sans CJK TC", "Noto Sans CJK JP", "Noto Sans CJK KR", "Noto Color Emoji"]
    }
}

/// How a note is set as a PDF, from the settings: the export sheet's
/// faces where it names one, else the note's, each with its Chinese face
/// behind it as the editor has it, and the accent's light shades.
pub fn pdf_look(cfg: &onyx_core::config::Config) -> onyx_core::pdf::Look {
    let (e, o) = (&cfg.editor, &cfg.pdf);
    let words = if o.face.is_empty() { text_family(&e.text_face, &e.text_custom) } else { text_family(&o.face, "") };
    let code = if o.mono.is_empty() { mono_family(&e.mono_face, &e.mono_custom) } else { mono_family(&o.mono, "") };
    let serif = is_serif(&words);
    let behind = |serif: bool| {
        let own = (!e.cjk_custom.trim().is_empty()).then(|| e.cjk_custom.trim().to_string());
        own.into_iter().chain([cjk_family(&e.cjk_face, serif).to_string()]).collect::<Vec<_>>()
    };
    let accent = crate::look::accent(&cfg.app.accent);
    onyx_core::pdf::Look {
        words: std::iter::once(words.to_string()).chain(behind(serif)).collect(),
        code: std::iter::once(code.to_string()).chain(behind(false)).collect(),
        fallbacks: system_fallbacks(serif).iter().map(|f| f.to_string()).collect(),
        accent: accent.light[0].to_string(),
        link: accent.link[0].to_string(),
    }
}

/// Load the carried faces. Called once at start.
pub fn install(cx: &mut App) {
    onyx_core::pdf::carry(FACES);
    let faces = FACES.iter().map(|b| Cow::Borrowed(*b)).collect();
    if let Err(e) = cx.text_system().add_fonts(faces) {
        eprintln!("onyx: the carried faces did not load: {e}");
    }
}

/// The family a note's words are set in: a font of the machine's named
/// by the person, else the face by the settings' key; an unknown key is
/// Inter.
pub fn text_family(key: &str, custom: &str) -> SharedString {
    if !custom.trim().is_empty() {
        return custom.trim().to_string().into();
    }
    SERIFS.iter().chain(SANS).find(|f| f.key == key).unwrap_or(&SANS[0]).family.into()
}

/// The family code is set in, the same way; an unknown key is JetBrains Mono.
pub fn mono_family(key: &str, custom: &str) -> SharedString {
    if !custom.trim().is_empty() {
        return custom.trim().to_string().into();
    }
    MONOS.iter().find(|f| f.key == key).unwrap_or(&MONOS[0]).family.into()
}

/// The app's name's face. Optima has only regular and bold, so the
/// wordmark draws its regular twice half a pixel apart for a weight
/// between (`widgets::wordmark`). Asked of the text system once: listing
/// every installed face takes about a tenth of a second on a Mac with
/// many fonts, and asked at every frame it made every interaction wait
/// that long.
pub fn wordmark_family(cx: &App) -> SharedString {
    static FAMILY: std::sync::OnceLock<SharedString> = std::sync::OnceLock::new();
    FAMILY
        .get_or_init(|| {
            let names = cx.text_system().all_font_names();
            if names.iter().any(|n| n == "Optima") { "Optima".into() } else { "Belleza".into() }
        })
        .clone()
}
