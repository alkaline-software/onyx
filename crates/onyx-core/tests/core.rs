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
