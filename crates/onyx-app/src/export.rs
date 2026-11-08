//! Export: a sheet with the note as it will come out on one side and how
//! it is set on the other, as a PDF, an image, a Word document or a web
//! page, the formats Craft offers less those that are the note itself. A
//! change lays the note out again off the window's thread and the page
//! fades to the new one, resizing over a moment when the paper changes. A
//! PDF and an image are written from the document the preview drew, so
//! what shows is what is made; Word and the web page are written from the
//! note, and the preview shows them as the PDF sets them. The note itself
//! is never written. `docs/export.md`.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use onyx_core::config::{Config, PdfConfig};
use onyx_core::pdf::{self, Laid};
use onyx_core::{paths, vault};

use crate::i18n::{phrase, t, tf};
use crate::menu::Entry;
use crate::shell::{Shell, EXPORT_CONTEXT};
use crate::ui_state::UiState;
use crate::widgets::{float_shadow, kbd_key, small_pill, swallow_click, Press, SLIDE};

actions!(onyx_export, [ExportNow, PagePrev, PageNext]);

/// The sheet at its largest, and what is kept clear round it.
const SHEET_W: f32 = 920.;
const SHEET_H: f32 = 680.;
const SHEET_CLEAR: f32 = 24.;
/// Clear of the traffic lights, however small the window.
const SHEET_TOP: f32 = 44.;
const SIDE_W: f32 = 352.;
/// The room round the page, and the row under it.
const PAGE_PAD: f32 = 28.;
const PAGER_H: f32 = 44.;
/// A run of presses is one layout.
const SETTLE: Duration = Duration::from_millis(40);

/// The formats, in the order they are offered: key, name, extension.
const FORMATS: &[(&str, &str, &str)] = &[("pdf", "PDF", "pdf"), ("image", phrase("Image"), "png"), ("word", "Word", "docx"), ("html", "HTML", "html")];

/// An image's pixels to a point of the page: as a Retina screen shows it.
const IMAGE_PIXELS: f32 = 2.;

fn extension_of(format: &str) -> &'static str {
    FORMATS.iter().find(|f| f.0 == format).map_or("pdf", |f| f.2)
}

/// The papers, in the order they are offered, each by its name.
const PAPERS: &[(&str, &str)] = &[("a4", "A4"), ("letter", phrase("Letter")), ("legal", phrase("Legal")), ("a5", "A5"), ("a3", "A3"), ("tabloid", phrase("Tabloid"))];

/// A page drawn, with its size in points.
struct Drawn {
    image: Arc<RenderImage>,
    size: (f32, f32),
    /// The drawing it was, so its fade plays once.
    serial: u64,
}

pub(crate) struct ExportSheet {
    path: PathBuf,
    title: String,
    text: Arc<String>,
    vault: Option<PathBuf>,
    /// The latest layout asked for; an answer to an older one is dropped.
    serial: u64,
    laid: Option<(u64, Arc<Laid>)>,
    error: Option<String>,
    page: usize,
    /// The latest drawing asked for, the page on show, and the one under
    /// it while that one fades in.
    drawing: u64,
    shown: Option<Drawn>,
    under: Option<Drawn>,
    /// How large the page stands: the room for it in pixels, and the
    /// screen's pixels to a point of them.
    room: (f32, f32),
    scale: f32,
    scroll: ScrollHandle,
    /// The long page's own scroller.
    page_scroll: ScrollHandle,
}

/// The sheet's size in this window, and the room in it for the page.
fn sheet_size(window: &Window) -> (f32, f32) {
    let view = window.viewport_size();
    ((f32::from(view.width) - SHEET_CLEAR * 2.).min(SHEET_W), (f32::from(view.height) - SHEET_TOP - SHEET_CLEAR).min(SHEET_H))
}

fn page_room(window: &Window) -> (f32, f32) {
    let (w, h) = sheet_size(window);
    ((w - SIDE_W - PAGE_PAD * 2.).max(80.), (h - PAGE_PAD - PAGER_H).max(80.))
}

/// How large the page stands in the room: whole, or for a page as long
/// as the note, as wide as the room, the rest a scroll away.
fn fit(size: (f32, f32), room: (f32, f32), long: bool) -> f32 {
    if long {
        room.0 / size.0
    } else {
        (room.0 / size.0).min(room.1 / size.1)
    }
}

/// The paper the sheet's options name.
fn paper_of(o: &PdfConfig) -> String {
    if !o.paper.is_empty() {
        return o.paper.clone();
    }
    pdf::paper_of_locale(&sys_locale::get_locale().unwrap_or_default()).to_string()
}

/// A paper's size in points, with the options' orientation.
fn paper_size(o: &PdfConfig) -> (f32, f32) {
    let key = paper_of(o);
    let (_, w, h) = pdf::PAPERS.iter().find(|p| p.0 == key).copied().unwrap_or(pdf::PAPERS[1]);
    if o.landscape { (h as f32, w as f32) } else { (w as f32, h as f32) }
}

/// A face's name for its pill: the one chosen, or that it is the note's.
fn face_name(key: &str, faces: &[&'static [crate::fonts::Face]]) -> SharedString {
    faces.iter().flat_map(|f| f.iter()).find(|f| f.key == key).map_or_else(|| t("As in the note").into(), |f| f.name.into())
}

impl ExportSheet {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Shell {
    /// The sheet over the note: a note open in a tab is set as it stands,
    /// changes and all, and any other as the disk has it.
    pub(crate) fn open_export(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // A note only: an asset has its own app for paper.
        if !onyx_core::vault::is_note(&path) {
            return;
        }
        if let Some(text) = self.text_of_tab(&path, cx) {
            self.show_export(path, text, window, cx);
            return;
        }
        let from = path.clone();
        let read = cx.background_spawn(async move { vault::read(&from) });
        cx.spawn_in(window, async move |this, cx| {
            let text = read.await;
            let _ = this.update_in(cx, |s, window, cx| match text {
                Ok(text) => s.show_export(path, text, window, cx),
                Err(e) => s.say(tf("The note could not be read: {why}", &[("why", &e.to_string())]), cx),
            });
        })
        .detach();
    }

    fn show_export(&mut self, path: PathBuf, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.export = Some(ExportSheet {
            title: vault::title(&path),
            vault: self.vault_root().map(Path::to_path_buf),
            path,
            text: Arc::new(text),
            serial: 0,
            laid: None,
            error: None,
            page: 0,
            drawing: 0,
            shown: None,
            under: None,
            room: page_room(window),
            scale: window.scale_factor(),
            scroll: ScrollHandle::new(),
            page_scroll: ScrollHandle::new(),
        });
        window.focus(&self.export_focus, cx);
        self.lay_out_export(cx);
        cx.notify();
    }

    pub(crate) fn close_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sheet) = self.export.take() {
            for drawn in sheet.shown.into_iter().chain(sheet.under) {
                cx.drop_image(drawn.image, Some(window));
            }
        }
        cx.background_spawn(async { pdf::forget() }).detach();
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    /// An option changed: kept, and the note laid out with it.
    pub(crate) fn set_export(&mut self, change: impl FnOnce(&mut PdfConfig), cx: &mut Context<Self>) {
        change(&mut self.cfg.pdf);
        let kept = self.cfg.pdf.clone();
        if let Err(e) = Config::edit(|c| c.pdf = kept) {
            eprintln!("onyx: the settings were not written: {e}");
        }
        self.lay_out_export(cx);
        cx.notify();
    }

    fn lay_out_export(&mut self, cx: &mut Context<Self>) {
        let look = crate::fonts::pdf_look(&self.cfg);
        let options = self.cfg.pdf.clone();
        let Some(sheet) = self.export.as_mut() else { return };
        sheet.serial += 1;
        let serial = sheet.serial;
        let (text, title, path, vault) = (sheet.text.clone(), sheet.title.clone(), sheet.path.clone(), sheet.vault.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let current = this.update(cx, |s, _| s.export.as_ref().is_some_and(|e| e.serial == serial)).unwrap_or(false);
            if !current {
                return;
            }
            let paper = paper_of(&options);
            let laid = cx.background_spawn(async move { pdf::lay_out(&pdf::Note { text: &text, title: &title, path: &path, vault: vault.as_deref() }, &look, &options, &paper) }).await;
            let _ = this.update(cx, |s, cx| {
                let Some(sheet) = s.export.as_mut().filter(|e| e.serial == serial) else { return };
                match laid {
                    Ok(laid) => {
                        sheet.page = sheet.page.min(laid.pages().saturating_sub(1));
                        sheet.laid = Some((serial, laid));
                        sheet.error = None;
                        s.draw_export(cx);
                    }
                    Err(e) => sheet.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The page showing, drawn at the screen's own pixels.
    fn draw_export(&mut self, cx: &mut Context<Self>) {
        let Some(sheet) = self.export.as_mut() else { return };
        let Some((_, laid)) = sheet.laid.clone() else { return };
        let page = sheet.page;
        let Some(size) = laid.size(page) else { return };
        sheet.drawing += 1;
        let drawing = sheet.drawing;
        let pixels = fit(size, sheet.room, self.cfg.pdf.long()) * sheet.scale;
        cx.spawn(async move |this, cx| {
            let image = cx
                .background_spawn(async move {
                    let mut picture = laid.draw(page, pixels)?;
                    // gpui's images are blue first.
                    for pixel in picture.rgba.chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                    let buffer = image::RgbaImage::from_raw(picture.width, picture.height, picture.rgba)?;
                    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
                })
                .await;
            let Some(image) = image else { return };
            let _ = this.update(cx, |s, cx| {
                let Some(sheet) = s.export.as_mut().filter(|e| e.drawing == drawing) else {
                    cx.drop_image(image, None);
                    return;
                };
                let old = sheet.under.take();
                sheet.under = sheet.shown.take();
                sheet.shown = Some(Drawn { image, size, serial: drawing });
                if let Some(old) = old {
                    cx.drop_image(old.image, None);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn turn_export(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(sheet) = self.export.as_mut() else { return };
        let Some((_, laid)) = &sheet.laid else { return };
        let to = sheet.page.saturating_add_signed(by).min(laid.pages().saturating_sub(1));
        if to != sheet.page {
            sheet.page = to;
            self.draw_export(cx);
            cx.notify();
        }
    }

    pub(crate) fn go_export_page(&mut self, page: usize, cx: &mut Context<Self>) {
        if let Some(sheet) = self.export.as_mut() {
            sheet.page = page;
        }
        self.draw_export(cx);
    }

    /// The document on show, once it is the latest.
    fn export_ready(&self) -> Option<Arc<Laid>> {
        let sheet = self.export.as_ref()?;
        sheet.laid.as_ref().filter(|(serial, _)| *serial == sheet.serial).map(|(_, laid)| laid.clone())
    }

    /// Export…: where to, from the system's panel. Cancelling it leaves
    /// the sheet as it was.
    pub(crate) fn export_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sheet) = self.export.as_ref() else { return };
        if self.export_ready().is_none() {
            return;
        }
        let folder = UiState::load().export_folder.filter(|f| f.is_dir()).or_else(|| sheet.path.parent().map(Path::to_path_buf)).unwrap_or_default();
        let ext = extension_of(&self.cfg.pdf.format);
        let chosen = cx.prompt_for_new_path(&folder, Some(&format!("{}.{ext}", sheet.title)));
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(path))) = chosen.await {
                let _ = this.update_in(cx, |s, window, cx| s.export_to(&path, window, cx));
            }
        })
        .detach();
    }

    pub(crate) fn export_to(&mut self, chosen: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(laid) = self.export_ready() else { return };
        let Some(sheet) = self.export.as_ref() else { return };
        let mut options = self.cfg.pdf.clone();
        // Word's page is the paper the preview shows, the region's when none is chosen.
        options.paper = paper_of(&options);
        let look = crate::fonts::pdf_look(&self.cfg);
        let out = pdf::finished(chosen, extension_of(&options.format));
        let (text, title, path, root) = (sheet.text.clone(), sheet.title.clone(), sheet.path.clone(), sheet.vault.clone());
        let mut ui = UiState::load();
        ui.export_folder = out.parent().map(Path::to_path_buf);
        ui.save_later();
        self.close_export(window, cx);
        let to = out.clone();
        let wrote = cx.background_spawn(async move {
            let note = pdf::Note { text: &text, title: &title, path: &path, vault: root.as_deref() };
            let bytes = match options.format.as_str() {
                "image" => laid.png(IMAGE_PIXELS)?,
                "word" => onyx_core::docx::write(&note, &look, &options)?,
                "html" => onyx_core::web::write(&note, &look, &options)?.into_bytes(),
                _ => laid.pdf()?,
            };
            paths::write_atomic(&to, &bytes).map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            let wrote = wrote.await;
            let _ = this.update(cx, |s, cx| match wrote {
                Ok(()) => {
                    let name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    s.say_done(tf("Exported {name}", &[("name", &name)]), out, cx);
                }
                Err(e) => s.say(tf("The file was not made: {why}", &[("why", &e)]), cx),
            });
        })
        .detach();
    }

    /// ⌘P: the note set as the sheet last set it, through the system's
    /// printing. A note open in a tab prints as it stands, changes and
    /// all; the note itself is not written.
    pub(crate) fn print_note(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // A note only: an asset has its own app for paper.
        if !onyx_core::vault::is_note(&path) {
            return;
        }
        if let Some(sheet) = self.export.as_ref().filter(|s| s.path == path) {
            if let Some(laid) = self.export_ready() {
                let title = sheet.title.clone();
                self.send_to_print(laid, title, window, cx);
            }
            return;
        }
        let look = crate::fonts::pdf_look(&self.cfg);
        let options = self.cfg.pdf.clone();
        let paper = paper_of(&options);
        let title = vault::title(&path);
        let root = self.vault_root().map(Path::to_path_buf);
        let held = self.text_of_tab(&path, cx);
        let name = title.clone();
        let laid = cx.background_spawn(async move {
            let text = match held {
                Some(text) => text,
                None => vault::read(&path).map_err(|e| e.to_string())?,
            };
            let laid = pdf::lay_out(&pdf::Note { text: &text, title: &name, path: &path, vault: root.as_deref() }, &look, &options, &paper);
            // The PDFs set after it are kept for the sheet's next layout;
            // a print has none.
            onyx_core::attach::forget();
            laid
        });
        cx.spawn_in(window, async move |this, cx| {
            let laid = laid.await;
            let _ = this.update_in(cx, |s, window, cx| match laid {
                Ok(laid) => s.send_to_print(laid, title, window, cx),
                Err(e) => s.say(tf("The note could not be printed: {why}", &[("why", &e)]), cx),
            });
        })
        .detach();
    }

    fn send_to_print(&mut self, laid: Arc<Laid>, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let size = laid.size(0).unwrap_or((612., 792.));
        let bytes = cx.background_spawn({
            let laid = laid.clone();
            async move { laid.pdf() }
        });
        cx.spawn_in(window, async move |this, cx| {
            let bytes = bytes.await;
            let _ = this.update_in(cx, |s, window, cx| match bytes {
                Ok(bytes) => {
                    if !crate::sys::print_pdf(window, &bytes, &title, size) {
                        s.say(tf("The note could not be printed: {why}", &[("why", &"PDFKit".to_string())]), cx);
                    }
                }
                Err(e) => s.say(tf("The note could not be printed: {why}", &[("why", &e)]), cx),
            });
        })
        .detach();
    }

    pub(crate) fn pick_export(&mut self, what: &'static str, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let id = id.to_string();
        match what {
            "pdf-paper" => self.set_export(|o| o.paper = id, cx),
            "pdf-face" => self.set_export(|o| o.face = id, cx),
            "pdf-mono" => self.set_export(|o| o.mono = id, cx),
            "pdf-format" => self.set_export(|o| o.format = id, cx),
            _ => {}
        }
        window.focus(&self.export_focus, cx);
    }

    /// A step the probe takes: `key:value` of an option.
    pub(crate) fn probe_export(&mut self, step: &str, cx: &mut Context<Self>) {
        let Some((key, value)) = step.split_once(':') else { return };
        let on = value == "on";
        let value = value.to_string();
        match key {
            "format" => self.set_export(|o| o.format = value, cx),
            "paper" => self.set_export(|o| o.paper = value, cx),
            "landscape" => self.set_export(|o| o.landscape = on, cx),
            "margins" => self.set_export(|o| o.margins = value, cx),
            "face" => self.set_export(|o| o.face = value, cx),
            "mono" => self.set_export(|o| o.mono = value, cx),
            "size" => self.set_export(|o| o.size = value.parse().unwrap_or(o.size), cx),
            "spacing" => self.set_export(|o| o.spacing = value, cx),
            "name" => self.set_export(|o| o.name = on, cx),
            "numbers" => self.set_export(|o| o.numbers = on, cx),
            "contents" => self.set_export(|o| o.contents = on, cx),
            "color" => self.set_export(|o| o.color = on, cx),
            "breaks" => self.set_export(|o| o.breaks = on, cx),
            "attached" => self.set_export(|o| o.attached = on, cx),
            _ => {}
        }
    }

    /// A pill that drops a menu: what is chosen, and a chevron.
    fn export_pill(&self, id: &'static str, words: SharedString, what: &'static str, entries: Vec<Entry>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        h_flex()
            .id(id)
            .h(crate::settings::CONTROL_H)
            .max_w(px(168.))
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
                cx.stop_propagation();
                this.menus.show(what, ev.position() + point(px(-100.), px(14.)), entries.clone());
                cx.notify();
            }))
            .child(div().truncate().child(words))
            .child(Icon::default().path("icons/chevron-down.svg").with_size(px(12.)).text_color(theme.muted_foreground))
            .into_any_element()
    }

    fn export_switch(&self, id: &'static str, on: bool, set: fn(&mut PdfConfig, bool), cx: &mut Context<Self>) -> AnyElement {
        self.motion.switch(id, on, Rc::new(move |this: &mut Self, on, _, cx| this.set_export(|o| set(o, on), cx)), cx)
    }

    fn export_choice(&self, id: &'static str, choices: Vec<(&'static str, &'static str, SharedString)>, now: &str, set: fn(&mut PdfConfig, &'static str), cx: &mut Context<Self>) -> AnyElement {
        let current = choices.iter().find(|c| c.0 == now).map_or(choices[0].0, |c| c.0);
        div().w(px(186.)).child(self.motion.segmented(id, choices, current, true, false, Rc::new(move |this: &mut Self, key, _, cx| this.set_export(|o| set(o, key), cx)), cx)).into_any_element()
    }

    /// How the note is set: what the page is, what the words are in, and
    /// what stands on it. What people change most comes first.
    fn export_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::fonts::{MONOS, SANS, SERIFS};
        use crate::settings::{compact_line as row, group};
        let theme = cx.theme().clone();
        let o = self.cfg.pdf.clone();
        let paper_now = paper_of(&o);
        let papers = PAPERS.iter().map(|(key, name)| Entry::Choice { id: (*key).into(), icon: "icons/file.svg", label: t(name).into(), picture: false, chosen: *key == paper_now }).collect();
        let face_entries = |now: &str, groups: &[&'static [crate::fonts::Face]]| {
            let mut entries = vec![Entry::Choice { id: "".into(), icon: "icons/file-text.svg", label: t("As in the note").into(), picture: false, chosen: now.is_empty() }];
            for faces in groups {
                entries.push(Entry::Rule);
                entries.extend(faces.iter().map(|f| Entry::Choice { id: f.key.into(), icon: "icons/typography.svg", label: f.name.into(), picture: false, chosen: f.key == now }));
            }
            entries
        };
        let page = vec![
            row(t("Page size").into(), self.export_pill("pdf-paper", t(PAPERS.iter().find(|p| p.0 == paper_now).map_or("A4", |p| p.1)).into(), "pdf-paper", papers, cx)),
            row(t("Orientation").into(), self.export_choice("pdf-turn", vec![("portrait", "", t("Portrait").into()), ("landscape", "", t("Landscape").into())], if o.landscape { "landscape" } else { "portrait" }, |o, k| o.landscape = k == "landscape", cx)),
            row(t("Margins").into(), self.export_choice("pdf-margins", vec![("small", "", t("Small").into()), ("normal", "", t("Normal").into()), ("large", "", t("Large").into())], &o.margins, |o, k| o.margins = k.into(), cx)),
        ];
        let size = self.stepper_with(
            "pdf-size",
            o.size,
            (o.size - 0.5).max(PdfConfig::LEAST_SIZE),
            (o.size + 0.5).min(PdfConfig::MOST_SIZE),
            Rc::new(|this: &mut Self, to, cx| this.set_export(|o| o.size = to, cx)),
            cx,
        );
        let kind = vec![
            row(t("Font").into(), self.export_pill("pdf-face", face_name(&o.face, &[SERIFS, SANS]), "pdf-face", face_entries(&o.face, &[SERIFS, SANS]), cx)),
            row(t("Code font").into(), self.export_pill("pdf-mono", face_name(&o.mono, &[MONOS]), "pdf-mono", face_entries(&o.mono, &[MONOS]), cx)),
            row(t("Font size").into(), size),
            row(t("Line spacing").into(), self.export_choice("pdf-spacing", vec![("tight", "", t("Tight").into()), ("normal", "", t("Normal").into()), ("loose", "", t("Loose").into())], &o.spacing, |o, k| o.spacing = k.into(), cx)),
        ];
        let format = o.format.as_str();
        let mut include = vec![row(t("Note's name").into(), self.export_switch("pdf-name", o.name, |o, on| o.name = on, cx))];
        if matches!(format, "pdf" | "word") {
            include.push(row(t("Page numbers").into(), self.export_switch("pdf-numbers", o.numbers, |o, on| o.numbers = on, cx)));
        }
        include.push(row(t("Contents").into(), self.export_switch("pdf-contents", o.contents, |o, on| o.contents = on, cx)));
        include.push(row(t("Color").into(), self.export_switch("pdf-color", o.color, |o, on| o.color = on, cx)));
        if format == "pdf" {
            include.push(row(t("New page for each top heading").into(), self.export_switch("pdf-breaks", o.breaks, |o, on| o.breaks = on, cx)));
        }
        include.push(row(t("Linked files").into(), self.export_switch("pdf-attached", o.attached, |o, on| o.attached = on, cx)));
        // A web page has no paper: the reader's window is its width.
        let mut groups = Vec::new();
        if format != "html" {
            groups.push(group(Some(t("Page").into()), None, page, &theme));
        }
        groups.push(group(Some(t("Text").into()), None, kind, &theme));
        let include_title = if format == "pdf" || format == "word" { t("On the page") } else { t("Include") };
        groups.push(group(Some(include_title.into()), None, include, &theme));
        groups
    }

    /// What the sheet makes, across the side's width.
    fn export_formats(&self, cx: &mut Context<Self>) -> AnyElement {
        let now = FORMATS.iter().find(|f| f.0 == self.cfg.pdf.format).map_or("pdf", |f| f.0);
        let choices = FORMATS.iter().map(|(key, name, _)| (*key, "", SharedString::from(t(name)))).collect();
        div().px(px(16.)).pb(px(6.)).child(self.motion.segmented("pdf-format", choices, now, true, false, Rc::new(|this: &mut Self, key, _, cx| this.set_export(|o| o.format = key.into(), cx)), cx)).into_any_element()
    }

    /// The page, or the blank paper it will be while it is first laid
    /// out. A new page fades in over the last one, and the two go to its
    /// size together.
    fn export_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(sheet) = self.export.as_ref() else { return div().into_any_element() };
        let long = self.cfg.pdf.long();
        // While a long page is first laid out, the blank paper is as tall as a sheet.
        let to = sheet.shown.as_ref().map_or_else(|| paper_size(&self.cfg.pdf), |d| d.size);
        let from = sheet.under.as_ref().map_or(to, |d| d.size);
        let room = sheet.room;
        let (w0, h0) = (from.0 * fit(from, room, long), from.1 * fit(from, room, long));
        let (w1, h1) = (to.0 * fit(to, room, long), to.1 * fit(to, room, long));
        let paper = div().relative().w(px(w1)).h(px(h1)).bg(white()).overflow_hidden().rounded(px(2.)).border_1().border_color(theme.border).shadow(float_shadow(&theme));
        let picture = |d: &Drawn| img(d.image.clone()).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Fill);
        let Some(top) = sheet.shown.as_ref() else { return paper.into_any_element() };
        let serial = top.serial;
        let new = div().absolute().inset_0().child(picture(top)).with_animation(ElementId::NamedInteger("pdf-page".into(), serial), Animation::new(SLIDE).with_easing(ease_out_quint()), |d, t| d.opacity(t));
        paper
            .children(sheet.under.as_ref().map(picture))
            .child(new)
            .with_animation(ElementId::NamedInteger("pdf-paper".into(), serial), Animation::new(SLIDE).with_easing(ease_out_quint()), move |d, t| d.w(px(w0 + (w1 - w0) * t)).h(px(h0 + (h1 - h0) * t)))
            .into_any_element()
    }

    /// Which page shows, with a press each way; the error if there is one.
    fn export_pager(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(sheet) = self.export.as_ref() else { return div().into_any_element() };
        if let Some(error) = &sheet.error {
            return div().h(px(PAGER_H)).flex().items_center().px(px(16.)).text_size(px(12.5)).text_color(theme.danger).truncate().child(tf("The file was not made: {why}", &[("why", error)])).into_any_element();
        }
        // A long page has one page and is scrolled, not turned.
        if self.cfg.pdf.long() {
            return div().h(px(PAGE_PAD)).flex_shrink_0().into_any_element();
        }
        let pages = sheet.laid.as_ref().map_or(1, |(_, l)| l.pages());
        let turn = |id: &'static str, icon: &'static str, by: isize, able: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .size(px(26.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .when(able, |d| {
                    d.cursor_pointer().hover(|s| s.bg(theme.foreground.opacity(0.06))).on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.turn_export(by, cx);
                    }))
                })
                .child(Icon::default().path(icon).with_size(px(15.)).text_color(if able { theme.foreground } else { theme.muted_foreground.opacity(0.4) }))
        };
        h_flex()
            .h(px(PAGER_H))
            .flex_shrink_0()
            .gap(px(8.))
            .items_center()
            .justify_center()
            .child(turn("pdf-prev", "icons/chevron-left.svg", -1, sheet.page > 0, cx))
            .child(div().min_w(px(92.)).text_center().text_size(px(12.5)).text_color(theme.muted_foreground).child(tf("Page {page} of {pages}", &[("page", &(sheet.page + 1).to_string()), ("pages", &pages.to_string())])))
            .child(turn("pdf-next", "icons/chevron-right.svg", 1, sheet.page + 1 < pages, cx))
            .into_any_element()
    }

    pub(crate) fn render_export(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        // The room follows the window; the page is drawn again when it
        // would show too soft.
        let room = page_room(window);
        let scale = window.scale_factor();
        let sheet = self.export.as_mut()?;
        let grown = room.0 > sheet.room.0 * 1.15 || room.1 > sheet.room.1 * 1.15 || scale != sheet.scale;
        sheet.room = room;
        sheet.scale = scale;
        if grown {
            self.draw_export(cx);
        }
        let theme = cx.theme().clone();
        let (w, h) = sheet_size(window);
        let top = ((f32::from(window.viewport_size().height) - h) / 2.).max(SHEET_TOP);
        let sheet = self.export.as_ref()?;
        let title = sheet.title.clone();
        let scroll = sheet.scroll.clone();
        self.scrollers.name_over(&scroll);
        let ready = self.export_ready().is_some();
        let ground = theme.popover;
        let options = v_flex().id("pdf-options").size_full().overflow_y_scroll().track_scroll(&scroll).child(v_flex().px(px(16.)).pt(px(4.)).pb(px(16.)).gap(px(14.)).children(self.export_rows(cx)));
        let formats = self.export_formats(cx);
        let side = v_flex()
            .w(px(SIDE_W))
            .flex_shrink_0()
            .h_full()
            .border_l_1()
            .border_color(theme.border)
            .child(v_flex().flex_shrink_0().px(px(30.)).pt(px(18.)).pb(px(10.)).gap(px(2.)).child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(t("Export"))).child(div().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(title)))
            .child(formats)
            .child(div().relative().flex_1().min_h_0().child(options).child(crate::shell::edge_fade(&scroll, ground, true, px(0.))).child(crate::shell::edge_fade(&scroll, ground, false, px(0.))))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .px(px(16.))
                    .pt(px(10.))
                    .pb(px(14.))
                    .gap(px(6.))
                    .items_center()
                    .justify_end()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(kbd_key("Esc", &theme))
                    .child(small_pill("pdf-cancel", "icons/close.svg", t("Cancel"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| this.close_export(window, cx))))
                    .child(div().w(px(6.)))
                    .child(small_pill("pdf-print", "icons/printer.svg", t("Print…"), Press::Quiet, !ready, &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        if let Some(path) = this.export.as_ref().map(|s| s.path.clone()) {
                            this.print_note(path, window, cx);
                        }
                    })))
                    .child(div().w(px(6.)))
                    .child(kbd_key("↩", &theme))
                    .child(small_pill("pdf-go", "icons/file-export.svg", t("Export…"), Press::Primary, !ready, &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.export_now(window, cx);
                    }))),
            );
        let tint = crate::shell::page_ground(&theme);
        let page = self.export_page(cx);
        let holder = if self.cfg.pdf.long() {
            let page_scroll = sheet.page_scroll.clone();
            self.scrollers.name_over(&page_scroll);
            div().id("pdf-long").flex_1().min_h_0().overflow_y_scroll().track_scroll(&page_scroll).pt(px(PAGE_PAD)).px(px(PAGE_PAD)).child(div().flex().justify_center().child(page)).into_any_element()
        } else {
            div().flex_1().min_h_0().pt(px(PAGE_PAD)).px(px(PAGE_PAD)).flex().items_center().justify_center().child(page).into_any_element()
        };
        let stage = v_flex().flex_1().min_w_0().h_full().rounded_l(px(17.)).bg(tint).child(holder).child(self.export_pager(cx));
        Some(
            crate::widgets::fade_in(
                "export-sheet",
                div()
                    .id("sheet-scrim")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(theme.overlay)
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(top))
                    .on_click(cx.listener(|this, _, window, cx| this.close_export(window, cx)))
                    .child(
                        h_flex()
                            .id("sheet-card")
                            .occlude()
                            .key_context(EXPORT_CONTEXT)
                            .track_focus(&self.export_focus)
                            .on_action(cx.listener(|this, _: &crate::shell::Escape, window, cx| this.close_export(window, cx)))
                            .on_action(cx.listener(|this, _: &ExportNow, window, cx| this.export_now(window, cx)))
                            .on_action(cx.listener(|this, _: &PagePrev, _, cx| this.turn_export(-1, cx)))
                            .on_action(cx.listener(|this, _: &PageNext, _, cx| this.turn_export(1, cx)))
                            .on_click(|_, window, cx| swallow_click(window, cx))
                            .w(px(w))
                            .h(px(h))
                            .rounded(px(18.))
                            .overflow_hidden()
                            .bg(theme.popover)
                            .border_1()
                            .border_color(theme.border)
                            .shadow(float_shadow(&theme))
                            .child(stage)
                            .child(side),
                    ),
            )
            .into_any_element(),
        )
    }
}
