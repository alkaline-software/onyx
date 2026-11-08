//! The core's tests. Each points `ONYX_HOME` at a temp folder of its
//! own (`isolated`), so none reads or writes the person's files.

use std::sync::Mutex;

use onyx_core::{config::Config, paths, update};

/// Tests run on threads of one process and share its environment.
static ENV: Mutex<()> = Mutex::new(());

fn isolated() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("a temp folder");
    std::env::set_var("ONYX_HOME", dir.path());
    std::env::remove_var("ONYX_APPEARANCE");
    (dir, guard)
}

#[test]
fn a_missing_or_broken_file_is_the_defaults() {
    let (_dir, _g) = isolated();
    assert_eq!(Config::load(), Config::default());
    std::fs::write(paths::config_file(), "{ not json").unwrap();
    assert_eq!(Config::load(), Config::default());
}

#[test]
fn a_file_with_a_field_missing_still_loads() {
    let (_dir, _g) = isolated();
    std::fs::write(paths::config_file(), r#"{"app": {"accent": "jade"}}"#).unwrap();
    let cfg = Config::load();
    assert_eq!(cfg.app.accent, "jade");
    assert_eq!(cfg.app.appearance, "system");
}

#[test]
fn an_override_is_never_written_back() {
    let (_dir, _g) = isolated();
    std::env::set_var("ONYX_APPEARANCE", "dark");
    assert_eq!(Config::load().app.appearance, "dark");
    Config::edit(|c| c.app.accent = "rose".into()).unwrap();
    std::env::remove_var("ONYX_APPEARANCE");
    let cfg = Config::load();
    assert_eq!(cfg.app.appearance, "system");
    assert_eq!(cfg.app.accent, "rose");
}

#[test]
fn text_sizes_are_a_preset_or_the_persons_own() {
    use onyx_core::config::{EditorConfig, TextSizes};
    let mut e = EditorConfig::default();
    assert_eq!(e.text_sizes(), TextSizes::MEDIUM);
    e.text_size = "large".into();
    assert_eq!(e.text_sizes(), TextSizes::LARGE);
    e.text_size = "custom".into();
    assert_eq!(e.text_sizes(), TextSizes::MEDIUM, "custom with nothing set is medium");
    e.sizes = Some(TextSizes { note: 40., code: 3. });
    assert_eq!(e.text_sizes(), TextSizes { note: TextSizes::MOST, code: TextSizes::LEAST }, "a size out of range is held in");
    assert_eq!(TextSizes::step(16., true), 17.);
    assert_eq!(TextSizes::step(16.4, false), 16.);
    assert_eq!(TextSizes::step(TextSizes::MOST, true), TextSizes::MOST);
}

#[test]
fn a_write_leaves_no_temp_file() {
    let (dir, _g) = isolated();
    paths::write_json(&paths::state_dir().join("x.json"), &serde_json::json!({"a": 1})).unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path().join("state")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["x.json"]);
}

#[test]
fn the_version_is_read_off_the_redirect() {
    assert_eq!(update::version_from_location("https://github.com/o/r/releases/tag/v1.2.3").as_deref(), Some("1.2.3"));
    assert_eq!(update::version_from_location("https://github.com/o/r/releases/tag/1.2.3/").as_deref(), Some("1.2.3"));
    assert_eq!(update::version_from_location("https://github.com/o/r/releases"), None);
    assert!(update::is_newer("0.10.0", "0.9.9"));
    assert!(!update::is_newer("1.0", "1.0.0"));
}

#[test]
fn every_system_has_an_installer_name() {
    let name = update::asset_name().expect("this system has an installer");
    assert!(name.starts_with(update::APP), "{name}");
}

// -- the vault ---------------------------------------------------------------

use onyx_core::vault;

fn a_vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp folder");
    let r = dir.path();
    std::fs::create_dir_all(r.join("Projects/Onyx")).unwrap();
    std::fs::create_dir_all(r.join(".obsidian")).unwrap();
    std::fs::write(r.join("Note 10.md"), "# ten").unwrap();
    std::fs::write(r.join("Note 2.md"), "# two").unwrap();
    std::fs::write(r.join("about.md"), "").unwrap();
    std::fs::write(r.join("picture.png"), "").unwrap();
    std::fs::write(r.join(".hidden.md"), "").unwrap();
    std::fs::write(r.join("Projects/Onyx/Plan.md"), "").unwrap();
    std::fs::write(r.join("Projects/Plan.md"), "").unwrap();
    dir
}

#[test]
fn the_tree_has_folders_first_in_the_file_managers_order() {
    let dir = a_vault();
    let tree = vault::tree(dir.path());
    let names: Vec<String> = tree.children.iter().map(|e| format!("{}{}", e.name, if e.dir { "/" } else { "" })).collect();
    assert_eq!(names, ["Projects/", "about", "Note 2", "Note 10"]);
    let projects = &tree.children[0];
    assert_eq!(projects.children.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Onyx", "Plan"]);
    assert_eq!(vault::notes(dir.path()).len(), 5);
}

#[test]
fn a_new_vault_is_a_folder_and_a_taken_folder_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let v = vault::new_vault(dir.path(), " Notes ").unwrap();
    assert_eq!(v, dir.path().join("Notes"));
    assert!(v.is_dir());
    std::fs::write(v.join("a.md"), "hi").unwrap();
    let again = vault::new_vault(dir.path(), "Notes").unwrap();
    assert_eq!(again, v);
    assert!(v.join("a.md").is_file(), "a folder that is there is kept as it is");
    std::fs::write(dir.path().join("file"), "").unwrap();
    assert!(vault::new_vault(dir.path(), "file").is_err());
    assert!(vault::new_vault(dir.path(), "").is_err());
    assert!(vault::new_vault(dir.path(), "a/b").is_err());
}

#[test]
fn a_new_note_takes_the_first_free_name() {
    let dir = a_vault();
    let a = vault::new_note(dir.path(), "Untitled").unwrap();
    let b = vault::new_note(dir.path(), "Untitled").unwrap();
    assert_eq!(a.file_name().unwrap(), "Untitled.md");
    assert_eq!(b.file_name().unwrap(), "Untitled 1.md");
    assert_eq!(vault::new_folder(dir.path(), "Projects").unwrap().file_name().unwrap(), "Projects 1");
}

#[test]
fn a_rename_keeps_the_extension_and_refuses_a_taken_name() {
    let dir = a_vault();
    let renamed = vault::rename(&dir.path().join("about.md"), "About Onyx").unwrap();
    assert_eq!(renamed.file_name().unwrap(), "About Onyx.md");
    assert!(renamed.is_file());
    assert!(vault::rename(&renamed, "Note 2").is_err());
    assert!(vault::rename(&renamed, "").is_err());
    assert!(vault::rename(&renamed, "a/b").is_err());
    let folder = vault::rename(&dir.path().join("Projects"), "Work").unwrap();
    assert!(folder.join("Plan.md").is_file());
}

#[test]
fn a_wikilink_finds_the_nearest_note_of_that_name() {
    let dir = a_vault();
    let r = dir.path();
    let from = r.join("Projects/Onyx/Plan.md");
    assert_eq!(vault::resolve_link(r, &from, "Plan"), Some(r.join("Projects/Onyx/Plan.md")));
    assert_eq!(vault::resolve_link(r, &r.join("about.md"), "plan#Heading|alias"), Some(r.join("Projects/Plan.md")));
    assert_eq!(vault::resolve_link(r, &from, "Projects/Plan.md"), Some(r.join("Projects/Plan.md")));
    assert_eq!(vault::resolve_link(r, &from, "Nowhere"), None);
}

#[test]
fn the_assets_folder_stays_out_of_the_tree_and_lists_its_files() {
    let dir = a_vault();
    let r = dir.path();
    std::fs::create_dir_all(r.join(".assets/Scans")).unwrap();
    std::fs::write(r.join(".assets/b 10.png"), "").unwrap();
    std::fs::write(r.join(".assets/b 2.png"), "").unwrap();
    std::fs::write(r.join(".assets/Scans/a.pdf"), "").unwrap();
    std::fs::write(r.join(".assets/.DS_Store"), "").unwrap();
    std::fs::create_dir_all(r.join("assets")).unwrap();
    let names: Vec<_> = vault::tree(r).children.iter().map(|c| c.name.clone()).collect();
    assert!(!names.contains(&".assets".to_string()));
    assert!(names.contains(&"assets".to_string()));
    assert_eq!(vault::asset_files(r), vec![r.join(".assets/Scans/a.pdf"), r.join(".assets/b 2.png"), r.join(".assets/b 10.png")]);
}

#[test]
fn a_file_is_found_beside_the_note_then_in_the_assets_then_anywhere() {
    let dir = a_vault();
    let r = dir.path();
    let from = r.join("Projects/Onyx/Plan.md");
    std::fs::create_dir_all(r.join(".assets")).unwrap();
    std::fs::write(r.join(".assets/trip plan.pdf"), "").unwrap();
    std::fs::write(r.join("Projects/deep.png"), "").unwrap();
    assert_eq!(vault::find_file(Some(r), &from, "trip plan.pdf"), Some(r.join(".assets/trip plan.pdf")));
    assert_eq!(vault::find_file(Some(r), &from, "../../.assets/trip%20plan.pdf"), Some(r.join(".assets/trip plan.pdf")));
    assert_eq!(vault::find_file(Some(r), &from, "deep.png|300"), Some(r.join("Projects/deep.png")));
    assert_eq!(vault::find_file(Some(r), &from, "picture.png"), Some(r.join("picture.png")));
    assert_eq!(vault::find_file(Some(r), &from, "https://example.com/a.png"), None);
    assert_eq!(vault::find_file(Some(r), &from, "nowhere.png"), None);
}

#[test]
fn an_asset_takes_a_free_name_and_one_in_the_vault_stays_put() {
    let dir = a_vault();
    let r = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("scan (1).pdf");
    std::fs::write(&file, "pdf").unwrap();
    let a = vault::copy_asset(r, &file).unwrap();
    let b = vault::copy_asset(r, &file).unwrap();
    assert_eq!(a, r.join(".assets/scan (1).pdf"));
    assert_eq!(b, r.join(".assets/scan (1) 1.pdf"));
    assert_eq!(std::fs::read(&b).unwrap(), b"pdf");
    assert_eq!(vault::copy_asset(r, &r.join("picture.png")).unwrap(), r.join("picture.png"));
    let pasted = vault::add_asset(r, "Pasted image.png", b"png").unwrap();
    assert_eq!(pasted, r.join(".assets/Pasted image.png"));
    let leftovers = std::fs::read_dir(r.join(".assets")).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains(".tmp")).count();
    assert_eq!(leftovers, 0);
}

#[test]
fn assets_coming_in_at_once_each_take_their_own_name() {
    let dir = a_vault();
    let r = dir.path().to_path_buf();
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("scan.pdf");
    std::fs::write(&file, "pdf").unwrap();
    let threads: Vec<_> = (0..8).map(|_| {
        let (r, file) = (r.clone(), file.clone());
        std::thread::spawn(move || vault::copy_asset(&r, &file).unwrap())
    }).collect();
    let mut kept: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    kept.sort();
    kept.dedup();
    assert_eq!(kept.len(), 8);
    assert!(kept.iter().all(|p| std::fs::read(p).unwrap() == b"pdf"));
    assert_eq!(std::fs::read_dir(r.join(".assets")).unwrap().count(), 8);
}

#[test]
fn a_reference_is_standard_markdown_from_the_notes_folder() {
    let r = std::path::Path::new("/v");
    let note = r.join("旅行/Trip.md");
    assert_eq!(vault::reference(&note, &r.join(".assets/夏威夷 banner.png")), "![夏威夷 banner](../.assets/夏威夷%20banner.png)");
    assert_eq!(vault::reference(&note, &r.join(".assets/Plan (v2).pdf")), "![Plan (v2)](../.assets/Plan%20%28v2%29.pdf)");
    assert_eq!(vault::reference(&r.join("Top.md"), &r.join(".assets/a.jpg")), "![a](.assets/a.jpg)");
    assert_eq!(vault::decoded("my%20picture.png"), "my picture.png");
    assert_eq!(vault::decoded("100%.png"), "100%.png");
}

#[test]
fn a_note_is_written_whole_and_leaves_no_temp_file() {
    let dir = a_vault();
    let note = dir.path().join("about.md");
    vault::write(&note, "words").unwrap();
    assert_eq!(vault::read(&note).unwrap(), "words");
    let names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".tmp")).collect();
    assert!(names.is_empty(), "{names:?}");
}

#[test]
fn names_sort_as_the_file_manager_sorts_them() {
    use std::cmp::Ordering::*;
    assert_eq!(vault::natural_cmp("Note 2", "Note 10"), Less);
    assert_eq!(vault::natural_cmp("b", "A"), Greater);
    assert_eq!(vault::natural_cmp("x", "x"), Equal);
    assert_eq!(vault::natural_cmp("note", "note 1"), Less);
}

// -- a note as a PDF ------------------------------------------------------

mod pdf {
    use std::path::Path;

    use onyx_core::config::PdfConfig;
    use onyx_core::pdf::{self, Laid, Look, Note};

    fn carried() {
        static FACES: &[&[u8]] = &[
            include_bytes!("../../onyx-app/assets/fonts/Inter-Regular.ttf"),
            include_bytes!("../../onyx-app/assets/fonts/Inter-Bold.ttf"),
            include_bytes!("../../onyx-app/assets/fonts/Inter-SemiBold.ttf"),
            include_bytes!("../../onyx-app/assets/fonts/Inter-Italic.ttf"),
            include_bytes!("../../onyx-app/assets/fonts/JetBrainsMono-Regular.ttf"),
            include_bytes!("../../onyx-app/assets/fonts/NotoSansSC-Regular.otf"),
        ];
        pdf::carry(FACES);
    }

    fn look() -> Look {
        Look { words: vec!["Inter".into(), "Noto Sans SC".into()], code: vec!["JetBrains Mono".into(), "Noto Sans SC".into()], fallbacks: vec![], accent: "#277580".into(), link: "#1F6F79".into() }
    }

    fn laid_in(dir: &Path, text: &str, o: &PdfConfig) -> Result<std::sync::Arc<Laid>, String> {
        carried();
        let path = dir.join("Field Notes.md");
        pdf::lay_out(&Note { text, title: "Field Notes", path: &path, vault: Some(dir) }, &look(), o, if o.paper.is_empty() { "a4" } else { &o.paper })
    }

    fn laid(text: &str, o: &PdfConfig) -> Result<std::sync::Arc<Laid>, String> {
        laid_in(tempfile::tempdir().expect("a temp folder").path(), text, o)
    }

    /// How many pages a note comes to: Typst refusing it fails the test.
    fn pages(text: &str) -> usize {
        laid(text, &PdfConfig::default()).expect("the note is set").pages()
    }

    #[test]
    fn a_note_with_everything_in_it_is_made() {
        let text = "---\ntags: [a]\n---\nWords, **bold**, *italic*, ~~gone~~, ==marked==, `code`, [a link](https://example.com), [[A Note|words]]. 中文也在。[^1]\n\n# One\n\n$e^{i\\pi} + 1 = 0$\n\n$$\n\\frac{a}{b}\n$$\n\n- [ ] open\n- [x] done\n  - under it\n- plain\n\n1. first\n2. second\n\n> A quote.\n\n| Name | N |\n|:---|--:|\n| a | 1 |\n\n```rust\nfn main() {}\n```\n\n---\n\n<div>html</div>\n\n[^1]: A note under the page.\n";
        let done = laid(text, &PdfConfig::default()).expect("the note is set");
        let bytes = done.pdf().expect("the file is written");
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn a_pdfs_words_are_found_on_their_pages() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let text = "Alpha beta gamma.\n\n<div style=\"page-break-after: always\"></div>\n\n".to_string() + &"Filler line.\n\n".repeat(80) + "Second Zebra here.\n";
        let done = laid_in(dir.path(), &text, &PdfConfig::default()).expect("the note is set");
        let file = dir.path().join("out.pdf");
        std::fs::write(&file, done.pdf().expect("the file is written")).expect("written");
        let pages = onyx_core::preview::pdf_text(&file).expect("its text is read");
        assert!(pages.len() >= 2, "{} pages", pages.len());
        let first = onyx_core::preview::pdf_find(&pages, "beta gamma");
        assert_eq!(first.len(), 1);
        let (page, from, to) = first[0];
        assert_eq!(page, 0);
        let t = &pages[0];
        assert_eq!(t.glyphs[from].text, "b");
        assert_eq!(t.glyphs[to].text, "a");
        // Its place is on the page, one line high, left to right.
        let boxes = t.boxes(from, to);
        assert_eq!(boxes.len(), 1);
        let (x, y, w, h) = boxes[0];
        assert!(x > 0. && y > 0. && w > 10. && h > 4. && x + w < t.size.0 && y + h < t.size.1, "{:?} on {:?}", boxes[0], t.size);
        // Any case, and on a later page.
        let zebra = onyx_core::preview::pdf_find(&pages, "zebra");
        assert_eq!(zebra.len(), 1);
        assert!(zebra[0].0 >= 1);
    }

    #[test]
    fn a_note_cannot_run_typst() {
        // None of these is read as markup or code, or the note would fail
        // or a file would be reached for.
        let text = "#panic(\"no\") and #read(\"/etc/passwd\") and $ and *a and _b and [c and <d> and @e and \\f and ` and # and // and /* and =\n\n#import \"/x\": *\n\n# #panic(\"heading\")\n\n- #panic()\n\n| #panic() |\n|---|\n| [ |\n";
        assert_eq!(pages(text), 1);
    }

    #[test]
    fn a_formula_typst_refuses_does_not_lose_the_note() {
        assert_eq!(pages("Before.\n\n$\\frac{a}{$ and $\\badcommand{x}$ and $$x_$$ and $1)($\n\nAfter.\n"), 1);
    }

    #[test]
    fn a_picture_that_cannot_be_read_is_its_alt_text() {
        let dir = tempfile::tempdir().expect("a temp folder");
        std::fs::write(dir.path().join("broken.png"), b"not a picture").expect("written");
        let text = "![Gone](gone.png)\n\n![Far](https://example.com/a.png)\n\n![Broken](broken.png)\n\nWords.\n";
        assert_eq!(laid_in(dir.path(), text, &PdfConfig::default()).expect("the note is set").pages(), 1);
    }

    #[test]
    fn a_picture_left_out_leaves_the_others_their_own() {
        let dir = tempfile::tempdir().expect("a temp folder");
        std::fs::write(dir.path().join("broken.png"), b"not a picture").expect("written");
        std::fs::write(dir.path().join("good.png"), include_bytes!("../../onyx-app/assets/icon/icon-128.png")).expect("written");
        // The good one takes the broken one's number once that is left out.
        let text = "![B](broken.png)\n\n![G](good.png)\n";
        assert!(laid_in(dir.path(), text, &PdfConfig::default()).is_ok());
        // Two footnotes and the note, each with a picture, keep three.
        let text = "One[^a] and two[^b].\n\n![G](good.png)\n\n[^a]: ![B](broken.png)\n[^b]: ![G](good.png)\n";
        assert!(laid_in(dir.path(), text, &PdfConfig::default()).is_ok());
    }

    #[test]
    fn a_picture_by_its_bare_name_is_found_anywhere_in_the_vault() {
        let dir = tempfile::tempdir().expect("a temp folder");
        std::fs::create_dir_all(dir.path().join("attachments/deep")).expect("made");
        std::fs::write(dir.path().join("attachments/deep/my picture.png"), include_bytes!("../../onyx-app/assets/icon/icon-128.png")).expect("written");
        // One page of words either way; two only if the picture came in
        // and took the rest of the first.
        let filler = "Words.\n\n".repeat(30);
        let text = format!("{filler}![[my picture.png]]\n\n![A](my%20picture.png)\n\n{filler}");
        assert!(laid_in(dir.path(), &text, &PdfConfig::default()).expect("set").pages() >= 2);
    }

    #[test]
    fn the_options_change_the_page() {
        let mut o = PdfConfig { paper: "letter".into(), ..Default::default() };
        let (w, h) = laid("Words.", &o).expect("set").size(0).expect("a page");
        assert_eq!((w.round(), h.round()), (612., 792.));
        o.landscape = true;
        let (w, h) = laid("Words.", &o).expect("set").size(0).expect("a page");
        assert_eq!((w.round(), h.round()), (792., 612.));
        // Each top heading on a page of its own, the first where it stands.
        o = PdfConfig { breaks: true, ..Default::default() };
        assert_eq!(laid("# One\n\nA\n\n# Two\n\nB\n\n# Three\n", &o).expect("set").pages(), 3);
        // Every option, at its ends.
        for spacing in ["tight", "loose"] {
            for margins in ["small", "large"] {
                let o = PdfConfig { spacing: spacing.into(), margins: margins.into(), size: PdfConfig::MOST_SIZE, contents: true, color: false, numbers: false, name: false, ..Default::default() };
                assert!(laid("# One\n\n`code`\n\n```rust\nfn a() {}\n```\n", &o).is_ok());
            }
        }
    }

    #[test]
    fn a_page_is_drawn_as_it_will_print() {
        let done = laid("Words.", &PdfConfig::default()).expect("set");
        let picture = done.draw(0, 1.).expect("a picture");
        assert_eq!((picture.width, picture.height), (595, 842));
        assert_eq!(picture.rgba.len(), 595 * 842 * 4);
        assert!(picture.rgba[..4] == [255, 255, 255, 255], "the paper is white");
    }

    #[test]
    fn a_name_the_panel_never_asked_about_is_not_overwritten() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let asked = dir.path().join("Asked.pdf");
        assert_eq!(pdf::output_path(&asked), asked);
        std::fs::write(dir.path().join("Plain.pdf"), b"keep").expect("written");
        assert_eq!(pdf::output_path(&dir.path().join("Plain")), dir.path().join("Plain 1.pdf"));
        assert_eq!(pdf::output_path(&dir.path().join("v1.2")), dir.path().join("v1.2.pdf"));
    }
}

mod html_on_paper {
    use onyx_core::config::PdfConfig;
    use onyx_core::pdf::{self, Look, Note};

    fn laid(text: &str) -> Result<std::sync::Arc<pdf::Laid>, String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n.md");
        let note = Note { text, title: "n", path: &path, vault: None };
        let look = Look { words: vec!["Inter".into()], code: vec!["JetBrains Mono".into()], fallbacks: vec![], accent: "#0d9488".into(), link: "#0f766e".into() };
        pdf::lay_out(&note, &look, &PdfConfig::default(), "a4")
    }

    #[test]
    fn tags_among_the_words_are_set() {
        let text = "a <u>u</u> <b>b</b> <i>i</i> <s>s</s> <mark>m</mark> <kbd>k</kbd> <sub>2</sub> <sup>3</sup> <small>t</small> <a href=\"https://a.b\">l</a> <span style=\"color: red; background: yellow\">c</span> <!-- no --> <x>y</x> <b>open\n\nline<br>break\n";
        assert!(laid(text).is_ok());
    }

    #[test]
    fn a_block_of_html_is_set() {
        let text = "<table>\n<tr><th>A</th><th align=\"right\">B</th></tr>\n<tr><td>1<br>2</td><td><b>x</b></td></tr>\n</table>\n\n<div align=\"center\">\n<h2>Title</h2>\n<ul><li>a</li><li>b</li></ul>\n</div>\n\n<details><summary>More</summary>Inside</details>\n\n<script>alert(1)</script>\n\n<!-- nothing -->\n\n</div>\n";
        assert!(laid(text).is_ok());
    }

    #[test]
    fn a_table_with_a_persons_widths_is_set() {
        let text = format!("| a | b |\n|{}|{}|\n| 1 | 2 |\n", "-".repeat(30), "-".repeat(70));
        assert!(laid(&text).is_ok());
    }

    #[test]
    fn html_cannot_run_typst() {
        let text = "<span style=\"color: red\">#panic(\"no\")</span>\n\n<p>#panic(\"no\")</p>\n\n| #panic(\"x\") |\n|---|\n";
        assert!(laid(text).is_ok());
    }
}

mod writing {
    use onyx_core::rhythm::shares_of;

    #[test]
    fn a_separator_of_a_hundred_dashes_says_widths() {
        assert_eq!(shares_of(&format!("|{}|{}|", "-".repeat(40), "-".repeat(60))), Some(vec![40., 60.]));
        assert_eq!(shares_of(&format!("> | :{}: | {}: |", "-".repeat(40), "-".repeat(60))), Some(vec![40., 60.]));
        assert_eq!(shares_of("|---|---|"), None);
    }
}

#[test]
fn renaming_a_pasted_picture_follows_its_link_from_a_folder_with_an_emoji() {
    let dir = a_vault();
    let r = dir.path();
    std::fs::create_dir_all(r.join(".assets")).unwrap();
    std::fs::create_dir_all(r.join("资料🗂️")).unwrap();
    std::fs::write(r.join(".assets/Pasted image 20261104135838.png"), "png").unwrap();
    let note = r.join("资料🗂️/代码📝.md");
    let text = "# 代码📝\n\n一张图片：\n\n![Pasted image 20261104135838](../.assets/Pasted%20image%2020261104135838.png)\n";
    std::fs::write(&note, text).unwrap();
    let old = r.join(".assets/Pasted image 20261104135838.png");
    // Found as the picture's own path, not one through the note's folder,
    // so the window knows it for an asset and renames it as one.
    assert_eq!(vault::find_file(Some(r), &note, "../.assets/Pasted%20image%2020261104135838.png"), Some(old.clone()));
    let new = vault::asset_renamed(&old, "图片1").unwrap();
    let edits = vault::relink(r, &note, text, &old, &new);
    assert_eq!(vault::apply_edits(text, &edits), "# 代码📝\n\n一张图片：\n\n![图片1](../.assets/图片1.png)\n");
    vault::rename_asset(r, &old, &new, &[]).unwrap();
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "# 代码📝\n\n一张图片：\n\n![图片1](../.assets/图片1.png)\n");
}

#[test]
fn renaming_an_asset_follows_every_link_to_it() {
    let dir = a_vault();
    let r = dir.path();
    std::fs::create_dir_all(r.join(".assets")).unwrap();
    std::fs::create_dir_all(r.join("Trips")).unwrap();
    std::fs::write(r.join(".assets/Hawaii plan.pdf"), "pdf").unwrap();
    std::fs::write(r.join(".assets/map.png"), "png").unwrap();
    let note = r.join("Trips/Hawaii.md");
    let text = "\
# Hawaii

[Hawaii plan](../.assets/Hawaii%20plan.pdf#page=2) and [the plan](<../.assets/Hawaii plan.pdf>).

![[Hawaii plan.pdf|Plan]] and [[.assets/Hawaii plan.pdf]]

<a href=\"../.assets/Hawaii%20plan.pdf\">plan</a>

![map](../.assets/map.png) stays, and so does `[Hawaii plan](../.assets/Hawaii%20plan.pdf)` in code.
";
    std::fs::write(&note, text).unwrap();
    let other = r.join("Other.md");
    std::fs::write(&other, "See [Hawaii plan](.assets/Hawaii%20plan.pdf).\n").unwrap();
    let old = r.join(".assets/Hawaii plan.pdf");
    let new = vault::asset_renamed(&old, "Oahu (final)").unwrap();
    assert_eq!(new, r.join(".assets/Oahu (final).pdf"));
    // The open note's changes, from its own text, while the old file is there.
    let edits = vault::relink(r, &note, text, &old, &new);
    vault::rename_asset(r, &old, &new, &[note.clone()]).unwrap();
    assert!(new.is_file() && !old.exists());
    assert_eq!(
        vault::apply_edits(text, &edits),
        "\
# Hawaii

[Oahu (final)](../.assets/Oahu%20%28final%29.pdf#page=2) and [the plan](<../.assets/Oahu (final).pdf>).

![[Oahu (final).pdf|Plan]] and [[.assets/Oahu (final).pdf]]

<a href=\"../.assets/Oahu%20%28final%29.pdf\">plan</a>

![map](../.assets/map.png) stays, and so does `[Hawaii plan](../.assets/Hawaii%20plan.pdf)` in code.
"
    );
    // The open note's file is left for its editor; the other note is written.
    assert_eq!(std::fs::read_to_string(&note).unwrap(), text);
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "See [Oahu (final)](.assets/Oahu%20%28final%29.pdf).\n");
    std::fs::write(r.join(".assets/taken.pdf"), "").unwrap();
    assert!(vault::asset_renamed(&new, "taken").is_err());
    assert!(vault::asset_renamed(&new, "a/b").is_err());
}

#[test]
fn a_note_or_folder_is_made_only_under_a_free_name_typed_for_it() {
    let dir = a_vault();
    let r = dir.path();
    assert_eq!(vault::free_stem(r, "Untitled", false), "Untitled");
    std::fs::write(r.join("Untitled.md"), "").unwrap();
    assert_eq!(vault::free_stem(r, "Untitled", false), "Untitled 1");
    assert_eq!(vault::free_stem(r, "Projects", true), "Projects 1");
    // Nothing is made until a name is given, and a taken one is refused.
    let made = vault::create(r, " Trip plan ", false).unwrap();
    assert_eq!(made, r.join("Trip plan.md"));
    assert_eq!(std::fs::read_to_string(&made).unwrap(), "");
    assert!(vault::create(r, "Trip plan", false).is_err());
    assert!(vault::create(r, "about", false).is_err());
    assert!(vault::create(r, "", false).is_err());
    assert!(vault::create(r, "a/b", false).is_err());
    assert_eq!(vault::create(r, "Notes.markdown", false).unwrap(), r.join("Notes.markdown"));
    assert!(vault::create(r, "Projects", true).is_err());
    assert!(vault::create(r, "Work", true).unwrap().is_dir());
}

#[test]
fn moving_notes_and_folders_keeps_every_link_going_where_it_went() {
    let dir = a_vault();
    let r = dir.path();
    std::fs::create_dir_all(r.join(".assets")).unwrap();
    std::fs::write(r.join(".assets/map.png"), "png").unwrap();
    std::fs::create_dir_all(r.join("Trips/Hawaii")).unwrap();
    std::fs::create_dir_all(r.join("Archive")).unwrap();
    // A note that links out by paths from its own folder.
    let trip = r.join("Trips/Hawaii/Plan.md");
    std::fs::write(&trip, "![map](../../.assets/map.png) [about](../../about.md) [[Note 2]]\n").unwrap();
    // A note that links to it three ways.
    let index = r.join("Index.md");
    std::fs::write(&index, "[plan](Trips/Hawaii/Plan.md) [[Trips/Hawaii/Plan]] [[Plan]]\n").unwrap();
    // The folder moves into Archive, the note with it.
    let open: Vec<std::path::PathBuf> = Vec::new();
    let moves = vault::move_into(r, &[r.join("Trips")], &r.join("Archive"), &open).unwrap();
    assert_eq!(moves, vec![(r.join("Trips"), r.join("Archive/Trips"))]);
    let moved = r.join("Archive/Trips/Hawaii/Plan.md");
    assert_eq!(std::fs::read_to_string(&moved).unwrap(), "![map](../../../.assets/map.png) [about](../../../about.md) [[Note 2]]\n");
    assert_eq!(std::fs::read_to_string(&index).unwrap(), "[plan](Archive/Trips/Hawaii/Plan.md) [[Archive/Trips/Hawaii/Plan]] [[Plan]]\n");
    // One already there is passed by; a taken name refuses before anything moves.
    assert!(vault::move_into(r, &[r.join("Index.md")], r, &open).unwrap().is_empty());
    std::fs::write(r.join("Archive/Index.md"), "").unwrap();
    assert!(vault::move_into(r, &[index.clone()], &r.join("Archive"), &open).is_err());
    assert!(index.is_file());
    // A folder is never put into itself.
    assert!(vault::move_into(r, &[r.join("Archive")], &r.join("Archive/Trips"), &open).unwrap().is_empty());
    // An open note's changes come from its own text, and its file is left.
    let text = "[plan](Archive/Trips/Hawaii/Plan.md) typed and not saved\n";
    let edits = vault::relink_moves(r, &index, &index, text, &[(moved.clone(), r.join("Plan.md"))]);
    assert_eq!(vault::apply_edits(text, &edits), "[plan](Plan.md) typed and not saved\n");
}

mod documents {
    use std::io::Read as _;
    use std::path::Path;

    use onyx_core::config::PdfConfig;
    use onyx_core::pdf::{Look, Note};
    use onyx_core::{docx, web};

    const EVERYTHING: &str = "---\ntags: [a]\n---\n# Field Notes\n\nWords, **bold**, *italic*, ~~gone~~, ==marked==, `code`, [a link](https://example.com), [[A Note|words]], <u>under</u> & <b>\"quoted\"</b>.\nA second line.[^1]\n\n## Two\n\n$e^{i\\pi}$\n\n$$\n\\frac{a}{b}\n$$\n\n- [ ] open\n- [x] done\n  - under it\n\n3. third\n4. fourth\n\n> A quote.\n\n| Name | N |\n|:---|--:|\n| a | 1 |\n\n```rust\nfn main() {}\n```\n\n---\n\n![A dot](dot.png)\n\n<p align=\"center\"><img src=\"dot.png\" alt=\"Half\" width=\"50%\"></p>\n\n![Gone](missing.png)\n\n<script>alert(1)</script>\n\n[^1]: A note at the end.\n";

    /// A one-pixel PNG.
    const DOT: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0, 0x05, 0, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82];

    fn look() -> Look {
        Look { words: vec!["Inter".into(), "Noto Sans SC".into()], code: vec!["JetBrains Mono".into()], fallbacks: vec![], accent: "#277580".into(), link: "#1F6F79".into() }
    }

    fn with_note<T>(text: &str, f: impl FnOnce(&Note) -> T) -> T {
        let dir = tempfile::tempdir().expect("a temp folder");
        std::fs::write(dir.path().join("dot.png"), DOT).expect("written");
        let path = dir.path().join("Field Notes.md");
        f(&Note { text, title: "Field Notes", path: &path, vault: Some(dir.path()) })
    }

    fn bytes_of(zip: &[u8], name: &str) -> Option<Vec<u8>> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).expect("a zip");
        let mut file = archive.by_name(name).ok()?;
        let mut out = Vec::new();
        file.read_to_end(&mut out).expect("read");
        Some(out)
    }

    fn part(zip: &[u8], name: &str) -> Option<String> {
        bytes_of(zip, name).map(|b| String::from_utf8(b).expect("text"))
    }

    #[test]
    fn a_word_document_holds_the_note_and_its_picture() {
        let o = PdfConfig { contents: true, ..PdfConfig::default() };
        let bytes = with_note(EVERYTHING, |note| docx::write(note, &look(), &o)).expect("written");
        let document = part(&bytes, "word/document.xml").expect("the document");
        for words in ["Field Notes", "bold", "A second line.", "fn main() {}", "\\frac{a}{b}", "&amp; ", "&quot;quoted&quot;"] {
            assert!(document.contains(words), "{words} is in the document");
        }
        // The name is the note's own heading, not printed twice; the
        // script and the YAML header are left out.
        assert!(!document.contains("w:val=\"Title\""));
        assert!(!document.contains("alert") && !document.contains("tags:"));
        assert!(document.contains("<w:footnoteReference w:id=\"1\"/>"));
        assert!(part(&bytes, "word/footnotes.xml").expect("the footnotes").contains("A note at the end."));
        assert!(document.contains("TOC \\o"));
        // The picture goes in once though named twice; the missing one is
        // its words.
        assert_eq!(bytes_of(&bytes, "word/media/image1.png").as_deref(), Some(DOT));
        assert!(bytes_of(&bytes, "word/media/image2.png").is_none());
        assert_eq!(document.matches("<w:drawing>").count(), 2);
        assert!(document.contains(">Gone<"));
        assert!(part(&bytes, "word/numbering.xml").expect("the numbering").contains("<w:start w:val=\"3\"/>"));
    }

    #[test]
    fn a_word_document_is_set_on_the_chosen_paper() {
        let o = PdfConfig { paper: "letter".into(), landscape: true, numbers: true, ..PdfConfig::default() };
        let bytes = with_note("Words.", |note| docx::write(note, &look(), &o)).expect("written");
        let document = part(&bytes, "word/document.xml").expect("the document");
        assert!(document.contains("<w:pgSz w:w=\"15840\" w:h=\"12240\" w:orient=\"landscape\"/>"));
        assert!(part(&bytes, "word/footer1.xml").is_some());
        // The note opens without its name, so the name stands on top.
        assert!(document.contains("w:val=\"Title\""));
    }

    #[test]
    fn a_web_page_holds_the_note_and_its_picture() {
        let o = PdfConfig { contents: true, color: false, ..PdfConfig::default() };
        let page = with_note(EVERYTHING, |note| web::write(note, &look(), &o)).expect("written");
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("<title>Field Notes</title>"));
        assert!(page.contains("<strong>bold</strong>") && page.contains("<mark>marked</mark>") && page.contains("<s>gone</s>"));
        assert!(page.contains("<a href=\"https://example.com\">a link</a>"));
        assert!(page.contains("&amp; <strong>&quot;quoted&quot;</strong>"));
        assert!(page.contains("A second line.") && page.contains("<br>"));
        assert!(page.contains("<ol start=\"3\">"));
        assert!(page.contains("<input type=\"checkbox\" disabled checked>"));
        assert!(page.contains("src=\"data:image/png;base64,iVBORw0KGgo"));
        assert!(page.contains("style=\"width: 50%\"") && page.contains("text-align: center"));
        assert!(page.contains("<nav class=\"contents\">") && page.contains("href=\"#two\""));
        assert!(page.contains("id=\"note-1\"") && page.contains("href=\"#ref-1\""));
        assert!(!page.contains("alert(1)") && !page.contains("tags:"));
        // The note opens with its name, which is not printed twice.
        assert!(!page.contains("class=\"title\""));
    }

    #[test]
    fn a_name_without_its_extension_gets_it() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let chosen = dir.path().join("Notes");
        assert_eq!(docx::output_path(&chosen), dir.path().join("Notes.docx"));
        std::fs::write(dir.path().join("Notes.html"), "").expect("written");
        assert_eq!(web::output_path(&chosen), dir.path().join("Notes 1.html"));
        assert_eq!(web::output_path(Path::new("/x/Asked.html")), Path::new("/x/Asked.html"));
    }
}

// -- the files a note names, set after it ---------------------------------

mod attached {
    use std::path::Path;

    use onyx_core::attach::{self, Body};
    use onyx_core::config::PdfConfig;
    use onyx_core::pdf::{self, Look, Note};
    use onyx_core::{docx, web};

    fn carried() {
        static FACES: &[&[u8]] = &[
            include_bytes!("../../onyx-app/assets/fonts/Inter-Regular.ttf"),
            include_bytes!("../../onyx-app/assets/fonts/Inter-Bold.ttf"),
            include_bytes!("../../onyx-app/assets/fonts/Inter-SemiBold.ttf"),
            include_bytes!("../../onyx-app/assets/fonts/JetBrainsMono-Regular.ttf"),
        ];
        pdf::carry(FACES);
    }

    fn look() -> Look {
        Look { words: vec!["Inter".into()], code: vec!["JetBrains Mono".into()], fallbacks: vec![], accent: "#277580".into(), link: "#1F6F79".into() }
    }

    const MAIN: &str = "# Main\n\nSee [[Other]] and ![](files/two.pdf).\n\n![](files/dot.png)\n\nThe [code](files/run.py), [[Other|again]], a [gone](Gone.md) one, ![](files/song.mp3), and [me](Main.md).\n";

    /// A vault: the note, another note, a PDF of two pages, a picture,
    /// code and a sound.
    fn vault(dir: &Path) {
        carried();
        std::fs::create_dir_all(dir.join("files")).expect("made");
        let text = format!("Page one.\n\n{}\n\nPage two.\n", "Filler.\n\n".repeat(60));
        let two = pdf::lay_out(&Note { text: &text, title: "Two", path: &dir.join("Two.md"), vault: Some(dir) }, &look(), &PdfConfig { name: false, ..PdfConfig::default() }, "a4").expect("set");
        assert_eq!(two.pages(), 2);
        std::fs::write(dir.join("files/two.pdf"), two.pdf().expect("written")).expect("written");
        std::fs::write(dir.join("files/dot.png"), include_bytes!("../../onyx-app/assets/icon/icon-128.png")).expect("written");
        std::fs::write(dir.join("files/run.py"), "print('hi')\n").expect("written");
        std::fs::write(dir.join("files/song.mp3"), b"ID3").expect("written");
        std::fs::write(dir.join("Other.md"), "# Other\n\nIt names the [PDF](files/two.pdf) and [[Main]].\n").expect("written");
        std::fs::write(dir.join("Main.md"), MAIN).expect("written");
    }

    fn with_main<T>(f: impl FnOnce(&Note) -> T) -> T {
        let dir = tempfile::tempdir().expect("a temp folder");
        vault(dir.path());
        let path = dir.path().join("Main.md");
        f(&Note { text: MAIN, title: "Main", path: &path, vault: Some(dir.path()) })
    }

    #[test]
    fn the_files_named_come_once_in_order() {
        with_main(|note| {
            let got = attach::gather(note);
            let names: Vec<&str> = got.iter().map(|a| a.name.as_str()).collect();
            // No picture, sound, missing file or the note itself; Other once.
            assert_eq!(names, ["Other.md", "two.pdf", "run.py"]);
            assert!(matches!(got[0].body, Body::Note(_)) && got[0].title == "Other");
            assert!(matches!(got[1].body, Body::Pdf { pages: 2, .. }));
            assert!(matches!(&got[2].body, Body::File(t) if t.contains("print('hi')")));
        });
        // A note that names no file reads nothing.
        let dir = tempfile::tempdir().expect("a temp folder");
        let path = dir.path().join("Plain.md");
        assert!(attach::gather(&Note { text: "Words and [a site](https://example.com).", title: "Plain", path: &path, vault: Some(dir.path()) }).is_empty());
    }

    #[test]
    fn a_pdf_sets_them_after_the_note_with_ways_there_and_back() {
        with_main(|note| {
            let off = pdf::lay_out(note, &look(), &PdfConfig { attached: false, ..PdfConfig::default() }, "a4").expect("set");
            let on = pdf::lay_out(note, &look(), &PdfConfig::default(), "a4").expect("set");
            // The note, Other, the PDF's two pages, the code.
            assert_eq!(off.pages(), 1);
            assert_eq!(on.pages(), 5);
            let jumps = |bytes: &[u8]| bytes.windows(4).filter(|w| w == b"/XYZ").count();
            let (off, on) = (off.pdf().expect("written"), on.pdf().expect("written"));
            // A place for each part to begin and for each way back to
            // return to.
            assert!(jumps(&on) >= jumps(&off) + 6, "{} against {}", jumps(&on), jumps(&off));
            // A long page holds them all too.
            let long = pdf::lay_out(note, &look(), &PdfConfig { format: "image".into(), ..PdfConfig::default() }, "a4").expect("set");
            assert_eq!(long.pages(), 1);
        });
    }

    #[test]
    fn word_and_the_web_set_them_too() {
        with_main(|note| {
            let bytes = docx::write(note, &look(), &PdfConfig::default()).expect("written");
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("a zip");
            let mut document = String::new();
            std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").expect("the document"), &mut document).expect("read");
            for n in 0..3 {
                assert!(document.contains(&format!("w:name=\"_OnyxPart{n}\"")) && document.contains(&format!("w:anchor=\"_OnyxPart{n}\"")));
                assert!(document.contains(&format!("w:name=\"_OnyxFrom{n}\"")) && document.contains(&format!("w:anchor=\"_OnyxFrom{n}\"")));
            }
            // The PDF's two pages are pictures, after the dot.
            assert_eq!(document.matches("<w:drawing>").count(), 3);
            assert!(zip.by_name("word/media/image3.png").is_ok());

            let page = web::write(note, &look(), &PdfConfig::default()).expect("written");
            for n in 0..3 {
                assert!(page.contains(&format!("id=\"part-{n}\"")) && page.contains(&format!("href=\"#part-{n}\"")));
                assert!(page.contains(&format!("id=\"from-{n}\"")) && page.contains(&format!("href=\"#from-{n}\"")));
            }
            assert_eq!(page.matches("class=\"leaf\"").count(), 2);
            assert!(page.contains("print(&#39;hi&#39;)") || page.contains("print('hi')"));
            // Off, nothing is set after the note.
            let page = web::write(note, &look(), &PdfConfig { attached: false, ..PdfConfig::default() }).expect("written");
            assert!(!page.contains("class=\"part\""));
        });
    }
}
