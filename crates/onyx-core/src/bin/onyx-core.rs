//! The core from a terminal: each part run without the window, by a
//! person or an agent. A product adds a subcommand for each part it has.

use onyx_core::{config::Config, paths, pdf, update, vault};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("paths") => {
            println!("home    {}", paths::root().display());
            println!("config  {}", paths::config_file().display());
            println!("state   {}", paths::state_dir().display());
            println!("cache   {}", paths::cache_dir().display());
            0
        }
        Some("config") => {
            println!("{}", serde_json::to_string_pretty(&Config::load()).unwrap_or_default());
            0
        }
        Some("update") => match update::latest() {
            Ok(latest) => {
                let now = update::current_version();
                println!("this {now}, newest {latest}{}", if update::is_newer(&latest, now) { ", newer" } else { "" });
                0
            }
            Err(e) => {
                eprintln!("onyx-core: {e}");
                1
            }
        },
        Some("pdf") => pdf_of(&args[1..]),
        Some("vault") => {
            // The tree of a vault, as the window shows it, or a note made in it.
            let Some(root) = args.get(1).map(std::path::PathBuf::from) else {
                eprintln!("usage: onyx-core vault <folder> [new <name>]");
                std::process::exit(2);
            };
            if args.get(2).map(String::as_str) == Some("new") {
                match vault::new_note(&root, args.get(3).map_or("Untitled", String::as_str)) {
                    Ok(p) => {
                        println!("{}", p.display());
                        0
                    }
                    Err(e) => {
                        eprintln!("onyx-core: {e}");
                        1
                    }
                }
            } else {
                fn show(e: &vault::Entry, depth: usize) {
                    for c in &e.children {
                        println!("{}{}{}", "  ".repeat(depth), c.name, if c.dir { "/" } else { "" });
                        show(c, depth + 1);
                    }
                }
                let started = std::time::Instant::now();
                let tree = vault::tree(&root);
                show(&tree, 0);
                eprintln!("{} notes, read in {:?}", vault::notes(&root).len(), started.elapsed());
                0
            }
        }
        Some("--try-update") => {
            // The download and the swap on a throwaway copy of the bundle,
            // so the steps can be checked without replacing the installed app.
            let (Some(version), Some(bundle)) = (args.get(1), args.get(2)) else {
                eprintln!("usage: onyx-core --try-update <version> <copy of Onyx.app>");
                std::process::exit(2);
            };
            match update::download(version, &|done, total| eprint!("\r{done} of {}", total.map_or("?".into(), |t| t.to_string()))).and_then(|f| update::install_into(&f, Some(std::path::Path::new(bundle)))) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("\nonyx-core: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("usage: onyx-core paths | config | update | vault <folder> [new <name>] | --try-update <version> <bundle>");
            2
        }
    };
    std::process::exit(code);
}

/// A note set as a PDF with no window, in the carried faces under
/// `fonts`, with the settings file's options.
fn pdf_of(args: &[String]) -> i32 {
    let [note, out, fonts] = args else {
        eprintln!("usage: onyx-core pdf <note> <out.pdf> <fonts folder>");
        return 2;
    };
    let faces: Vec<&'static [u8]> = std::fs::read_dir(fonts).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "ttf" || x == "otf")).filter_map(|p| std::fs::read(p).ok()).map(|b| &*Vec::leak(b)).collect();
    pdf::carry(Vec::leak(faces));
    let path = std::path::Path::new(note);
    let Ok(text) = vault::read(path) else {
        eprintln!("onyx-core: {note} could not be read");
        return 1;
    };
    let title = vault::title(path);
    let look = pdf::Look { words: vec!["Inter".into(), "Noto Sans SC".into()], code: vec!["JetBrains Mono".into(), "Noto Sans SC".into()], fallbacks: vec![], accent: "#277580".into(), link: "#1F6F79".into() };
    let options = Config::load().pdf;
    let paper = if options.paper.is_empty() { "a4" } else { &options.paper };
    let started = std::time::Instant::now();
    let made = pdf::lay_out(&pdf::Note { text: &text, title: &title, path, vault: path.parent() }, &look, &options, paper).and_then(|laid| {
        let bytes = laid.pdf()?;
        paths::write_atomic(std::path::Path::new(out), &bytes).map_err(|e| e.to_string())?;
        Ok((laid.pages(), bytes.len()))
    });
    match made {
        Ok((pages, size)) => {
            println!("{pages} pages, {} KB, in {:?}", size / 1024, started.elapsed());
            0
        }
        Err(e) => {
            eprintln!("onyx-core: {e}");
            1
        }
    }
}
