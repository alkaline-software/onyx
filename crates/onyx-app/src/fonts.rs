//! The faces the window carries, and which face each place uses.
//!
//! The chrome is the system's face and is never set here. A note is set
//! in three faces of the person's choice, as emaki sets a conversation:
//! its English, which is every script but Chinese, Japanese and Korean
//! (`SERIFS`, `SANS`), its CJK (`CJK`), and its code (`MONOS`), the same
//! faces emaki offers under the same keys, each a font of the machine's
//! instead when the person chooses one (Custom). The two never share a
//! character: a run of text is cut where its writing changes and each
//! stretch set in its own face (`split_cjk`), so an English face with
//! Chinese in it sets none of that Chinese, and a face wanted for both is
//! chosen for both. The app's name (the
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
/// A CJK face's name in its own writing, its tile's sample.
pub fn cjk_native(key: &str) -> Option<&'static str> {
    match key {
        "noto-serif-sc" => Some("思源宋体"),
        "noto-sans-sc" => Some("思源黑体"),
        "wenkai" => Some("霞鹜文楷"),
        _ => None,
    }
}

/// The key that stands for the person's own face in each of the three.
pub const CUSTOM: &str = "custom";

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

/// The family a note's English is set in: the person's own under
/// `custom`, else the face by the settings' key; an unknown key, or
/// Custom with no face, is Inter.
pub fn text_family(key: &str, custom: &str) -> SharedString {
    if key == CUSTOM && !custom.trim().is_empty() {
        return custom.trim().to_string().into();
    }
    SERIFS.iter().chain(SANS).find(|f| f.key == key).unwrap_or(&SANS[0]).family.into()
}

/// The family code is set in, the same way; an unknown key is JetBrains Mono.
pub fn mono_family(key: &str, custom: &str) -> SharedString {
    if key == CUSTOM && !custom.trim().is_empty() {
        return custom.trim().to_string().into();
    }
    MONOS.iter().find(|f| f.key == key).unwrap_or(&MONOS[0]).family.into()
}

/// The family a note's Chinese, Japanese and Korean are set in: the
/// person's own under `custom`, else the carried face by its key, `auto`
/// a serif's behind a serif English face and a sans serif's otherwise.
pub fn cjk_text_family(key: &str, custom: &str, english: &str) -> SharedString {
    if key == CUSTOM && !custom.trim().is_empty() {
        return custom.trim().to_string().into();
    }
    cjk_family(key, is_serif(english))
}

/// What the CJK face lacks: the system's faces of its kind for Japanese,
/// Korean and the rest, and the system's emoji last.
pub fn cjk_behind(english: &str) -> gpui::FontFallbacks {
    gpui::FontFallbacks::from_fonts(system_fallbacks(is_serif(english)).iter().map(|f| f.to_string()).collect())
}

/// Whether a character is Chinese, Japanese or Korean writing, set in
/// the CJK face: Han in every block, kana, Hangul, Bopomofo, and the
/// punctuation, symbols and full-width forms made for them. Quotes and
/// dashes the scripts share stay in the English face.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x2FF0..=0x303F | 0x3040..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
        | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

/// Runs cut where the writing changes between CJK and the rest, a CJK
/// stretch set in `cjk` at its run's weight and slant, the rest as it
/// was: the English face never sets a CJK character, whatever it holds.
/// `runs` cover `text` byte for byte, as gpui's do.
pub fn split_cjk(text: &str, runs: Vec<gpui::TextRun>, cjk: &SharedString, behind: &Option<gpui::FontFallbacks>) -> Vec<gpui::TextRun> {
    if !text.chars().any(is_cjk) {
        return runs;
    }
    let mut out = Vec::with_capacity(runs.len() + 4);
    let mut at = 0;
    for run in runs {
        let end = (at + run.len).min(text.len());
        let Some(piece) = text.get(at..end) else {
            out.push(run);
            at = end;
            continue;
        };
        let mut from = 0;
        let mut kind: Option<bool> = None;
        for (i, c) in piece.char_indices() {
            let here = is_cjk(c);
            if kind.is_some_and(|k| k != here) {
                out.push(stretch(&run, i - from, kind == Some(true), cjk, behind));
                from = i;
            }
            kind = Some(here);
        }
        if piece.len() > from {
            out.push(stretch(&run, piece.len() - from, kind == Some(true), cjk, behind));
        }
        at = end;
    }
    out
}

thread_local! {
    /// The CJK face a note is set in and the faces behind it, for the
    /// toolkit's fields that show a note's words (its title), which cut
    /// their runs through `split_for_fields`; set with the editors' look.
    static FIELDS_CJK: std::cell::RefCell<(SharedString, Option<gpui::FontFallbacks>)> = std::cell::RefCell::new((SharedString::default(), None));
}

/// The CJK face the fields take from now on.
pub fn set_fields_cjk(family: SharedString, behind: Option<gpui::FontFallbacks>) {
    FIELDS_CJK.with(|f| *f.borrow_mut() = (family, behind));
}

/// A field's line cut as the editor cuts a note's (`split_cjk`), for
/// `InputBaseState::set_split_runs`.
pub fn split_for_fields(text: &str, runs: Vec<gpui::TextRun>) -> Vec<gpui::TextRun> {
    FIELDS_CJK.with(|f| {
        let (family, behind) = &*f.borrow();
        if family.is_empty() { runs } else { split_cjk(text, runs, family, behind) }
    })
}

fn stretch(run: &gpui::TextRun, len: usize, cjk: bool, family: &SharedString, behind: &Option<gpui::FontFallbacks>) -> gpui::TextRun {
    let mut run = run.clone();
    run.len = len;
    if cjk {
        run.font.family = family.clone();
        run.font.fallbacks = behind.clone();
        run.font.features = gpui::FontFeatures::default();
    }
    run
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

// -- the system's faces ------------------------------------------------------

/// The system's font families for the Custom tiles' list (Settings,
/// Fonts): all, those with Chinese, Japanese or Korean, the monospaced,
/// each sorted, and what a CJK family is called in its own writing.
#[derive(Default)]
pub struct Families {
    pub all: Vec<String>,
    pub cjk: Vec<String>,
    pub mono: Vec<String>,
    pub native: std::collections::HashMap<String, String>,
}

/// Which of the three a Custom tile chooses for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    English,
    Cjk,
    Mono,
}

impl Families {
    pub fn of(&self, slot: Slot) -> &[String] {
        match slot {
            Slot::English => &self.all,
            Slot::Cjk => &self.cjk,
            Slot::Mono => &self.mono,
        }
    }

    /// A family as the list shows it: its own name and the English one
    /// where it has both, "霞鹜文楷 - LXGW WenKai".
    pub fn shown(&self, family: &str) -> String {
        both_names(self.native.get(family).map(String::as_str), family)
    }

    /// The families whose names, either of them, hold every word typed.
    pub fn matching(&self, slot: Slot, query: &str) -> Vec<String> {
        let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
        self.of(slot).iter().filter(|f| {
            let names = format!("{} {}", f.to_lowercase(), self.native.get(*f).map(|n| n.to_lowercase()).unwrap_or_default());
            words.iter().all(|w| names.contains(w))
        }).cloned().collect()
    }
}

pub fn both_names(native: Option<&str>, name: &str) -> String {
    match native {
        Some(n) if n != name => format!("{n} - {name}"),
        _ => name.to_string(),
    }
}

/// Whether a family is one Onyx carries, which its tiles offer already.
fn ours(family: &str) -> bool {
    SERIFS.iter().chain(SANS).chain(MONOS).chain(CJK).any(|f| f.family == family) || family == "Belleza"
}

/// The system's families, read off the main thread: each family's first
/// face asked whether it is monospaced, whether it has 汉, あ or 한, and
/// where it has, what it is called in that writing. A face that cannot be
/// read is left out, not the list.
#[cfg(target_os = "macos")]
pub fn system_families() -> Families {
    use core_text::font_descriptor::SymbolicTraitAccessors as _;
    use core_text::font_descriptor::TraitAccessors as _;
    let Some(faces) = std::panic::catch_unwind(|| core_text::font_collection::create_for_all_families().get_descriptors()).ok().flatten() else { return Families::default() };
    let mut out = Families::default();
    let mut seen = std::collections::HashSet::new();
    for face in faces.iter() {
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let family = face.family_name();
            if family.starts_with('.') || ours(&family) || seen.contains(&family) {
                return None;
            }
            let mono = face.traits().symbolic_traits().is_monospace();
            let font = core_text::font::new_from_descriptor(&face, 12.);
            let cjk = ['汉', 'あ', '한'].iter().any(|c| {
                let mut unit = [0u16; 2];
                let unit = c.encode_utf16(&mut unit);
                let mut glyphs = [0u16; 2];
                unsafe { font.get_glyphs_for_characters(unit.as_ptr(), glyphs.as_mut_ptr(), unit.len() as isize) }
            });
            let native = if cjk { font.get_font_table(u32::from_be_bytes(*b"name")).and_then(|d| native_of(d.bytes())) } else { None };
            Some((family, mono, cjk, native))
        }));
        let Ok(Some((family, mono, cjk, native))) = read else { continue };
        seen.insert(family.clone());
        if mono {
            out.mono.push(family.clone());
        }
        if cjk {
            out.cjk.push(family.clone());
        }
        if let Some(n) = native {
            out.native.insert(family.clone(), n);
        }
        out.all.push(family);
    }
    for list in [&mut out.all, &mut out.cjk, &mut out.mono] {
        list.sort_by_cached_key(|f| f.to_lowercase());
    }
    out
}

/// Elsewhere every family the text system knows, in each list.
#[cfg(not(target_os = "macos"))]
pub fn system_families_of(names: Vec<String>) -> Families {
    let mut all: Vec<String> = names.into_iter().filter(|f| !f.starts_with('.') && !ours(f)).collect();
    all.sort_by_cached_key(|f| f.to_lowercase());
    all.dedup();
    Families { cjk: all.clone(), mono: all.clone(), all, native: Default::default() }
}

#[cfg(target_os = "macos")]
fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

/// A name of a `name` table by its id, in the first of `langs` it has.
#[cfg(target_os = "macos")]
fn name_of(name: &[u8], id: u16, langs: &[u16]) -> Option<String> {
    let (count, strings) = (u16_at(name, 2)? as usize, u16_at(name, 4)? as usize);
    let mut found: Vec<(usize, String)> = Vec::new();
    for ix in 0..count {
        let rec = 6 + ix * 12;
        let (platform, lang, nid) = (u16_at(name, rec)?, u16_at(name, rec + 4)?, u16_at(name, rec + 6)?);
        if nid != id || platform == 1 {
            continue;
        }
        let (len, at) = (u16_at(name, rec + 8)? as usize, strings + u16_at(name, rec + 10)? as usize);
        let Some(raw) = name.get(at..at + len) else { continue };
        let text = String::from_utf16_lossy(&raw.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect::<Vec<_>>());
        found.push((langs.iter().position(|l| *l == lang).unwrap_or(langs.len()), text));
    }
    found.into_iter().min_by_key(|(rank, _)| *rank).map(|(_, text)| text)
}

/// What a family is called in Chinese, Japanese or Korean where its file
/// says, the window's language first: none where that is the English.
#[cfg(target_os = "macos")]
fn native_of(name: &[u8]) -> Option<String> {
    const HANS: &[u16] = &[0x804, 0x1004];
    const HANT: &[u16] = &[0x404, 0xC04, 0x1404];
    const JA: &[u16] = &[0x411];
    const KO: &[u16] = &[0x412];
    let order: Vec<u16> = match crate::i18n::lang() {
        "zh-Hant" => [HANT, HANS, JA, KO].concat(),
        "ja" => [JA, HANS, HANT, KO].concat(),
        "ko" => [KO, HANS, HANT, JA].concat(),
        _ => [HANS, HANT, JA, KO].concat(),
    };
    let english = name_of(name, 16, &[0x409]).or_else(|| name_of(name, 1, &[0x409]))?;
    let native = name_of(name, 16, &order).filter(|n| *n != english).or_else(|| name_of(name, 1, &order))?;
    let cjk = native.chars().any(is_cjk);
    (cjk && native != english).then_some(native)
}

#[cfg(test)]
mod tests {
    // Named, not `super::*`: gpui's glob is not here, but keep it plain.
    use super::{is_cjk, split_cjk};
    use gpui::{black, Font, SharedString, TextRun};

    fn run(len: usize, family: &str) -> TextRun {
        TextRun { len, font: Font { family: family.to_string().into(), ..gpui::font("x") }, color: black(), background_color: None, underline: None, strikethrough: None }
    }

    #[test]
    fn english_and_cjk_take_their_own_faces() {
        assert!(is_cjk('汉') && is_cjk('あ') && is_cjk('한') && is_cjk('，') && is_cjk('。'));
        assert!(!is_cjk('a') && !is_cjk('“') && !is_cjk('—') && !is_cjk(' '));
        let text = "Hello 世界, ok";
        let cjk: SharedString = "LXGW WenKai".into();
        let out = split_cjk(text, vec![run(text.len(), "Inter")], &cjk, &None);
        let pieces: Vec<(usize, String)> = out.iter().map(|r| (r.len, r.font.family.to_string())).collect();
        assert_eq!(pieces, vec![(6, "Inter".into()), (6, "LXGW WenKai".into()), (4, "Inter".into())]);
        assert_eq!(out.iter().map(|r| r.len).sum::<usize>(), text.len());
        // Bold stays bold across the cut.
        let mut bold = run(text.len(), "Inter");
        bold.font.weight = gpui::FontWeight::BOLD;
        assert!(split_cjk(text, vec![bold], &cjk, &None).iter().all(|r| r.font.weight == gpui::FontWeight::BOLD));
    }
}
