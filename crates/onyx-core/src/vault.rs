//! A vault: a folder of markdown notes, as Obsidian has it. Nothing is
//! kept anywhere else: a note is a file, a folder is a folder, and the
//! tree is read off the disk whenever the disk changes (`watch`). A note
//! deleted goes to the system's trash, never straight off the disk.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::paths;

/// The extensions a note may have. New notes take the first.
pub const NOTE_EXTS: &[&str] = &["md", "markdown"];

/// Folders a vault keeps for itself, not shown in the tree.
const HIDDEN: &[&str] = &[".obsidian", ".onyx", ".trash", ".git"];

/// A folder or a note in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The name as the file manager shows it: a note without its extension.
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
    /// A folder's entries, folders first; empty for a note.
    pub children: Vec<Entry>,
}

pub fn is_note(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| NOTE_EXTS.iter().any(|n| n.eq_ignore_ascii_case(e)))
}

/// A note's name as the tree shows it: the file's stem.
pub fn title(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// The vault's tree: folders first, then notes, each in the order the
/// file manager uses (case folded, a number by its value, so "Note 2"
/// stands before "Note 10"). Hidden entries and the vault's own folders
/// are left out. A folder that cannot be read is empty.
pub fn tree(root: &Path) -> Entry {
    Entry { name: title_of_dir(root), path: root.to_path_buf(), dir: true, children: children(root) }
}

fn title_of_dir(dir: &Path) -> String {
    dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| dir.display().to_string())
}

fn children(dir: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Entry> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || HIDDEN.contains(&name.as_str()) {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            out.push(Entry { name, path: path.clone(), dir: true, children: children(&path) });
        } else if is_note(&path) {
            out.push(Entry { name: title(&path), path, dir: false, children: Vec::new() });
        }
    }
    out.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| natural_cmp(&a.name, &b.name)));
    out
}

/// The file manager's order: case folded, a run of digits by its value.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut xn = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    xn.push(c);
                    ai.next();
                }
                let mut yn = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    yn.push(c);
                    bi.next();
                }
                let (xv, yv) = (xn.trim_start_matches('0'), yn.trim_start_matches('0'));
                let by_value = xv.len().cmp(&yv.len()).then_with(|| xv.cmp(yv));
                if by_value != Ordering::Equal {
                    return by_value;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// Every note in the vault, in the tree's order, for a search or a link.
pub fn notes(root: &Path) -> Vec<PathBuf> {
    fn walk(e: &Entry, out: &mut Vec<PathBuf>) {
        for c in &e.children {
            if c.dir {
                walk(c, out);
            } else {
                out.push(c.path.clone());
            }
        }
    }
    let mut out = Vec::new();
    walk(&tree(root), &mut out);
    out
}

pub fn read(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

/// A note written whole: a temp file beside it and a rename, so a reader
/// (another app, a sync) sees the old note or the new one and never half.
pub fn write(path: &Path, text: &str) -> io::Result<()> {
    paths::write_atomic(path, text.as_bytes())
}

/// A name no entry of `dir` has yet: `stem`, then `stem 1`, `stem 2`…
fn free_name(dir: &Path, stem: &str, ext: Option<&str>) -> PathBuf {
    let with = |n: usize| {
        let base = if n == 0 { stem.to_string() } else { format!("{stem} {n}") };
        match ext {
            Some(e) => dir.join(format!("{base}.{e}")),
            None => dir.join(base),
        }
    };
    (0..).map(with).find(|p| !p.exists()).unwrap_or_else(|| with(0))
}

/// A new, empty note in `dir`, named `stem` or the first free name after it.
pub fn new_note(dir: &Path, stem: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = free_name(dir, stem, Some(NOTE_EXTS[0]));
    write(&path, "")?;
    Ok(path)
}

/// The first name free in `dir` for a new note (`stem`, `stem 1`…) or
/// folder, without making anything: what the window offers to be typed over.
pub fn free_stem(dir: &Path, stem: &str, folder: bool) -> String {
    let path = free_name(dir, stem, (!folder).then_some(NOTE_EXTS[0]));
    if folder { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() } else { title(&path) }
}

/// A note or a folder made in `dir` under the name typed for it: a note
/// takes `.md` unless the name has a note's extension. Refused when the
/// name is empty, has a separator, or is taken, so nothing is written
/// over and nothing is named other than as asked.
pub fn create(dir: &Path, name: &str, folder: bool) -> io::Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a name is one or more characters with no slash"));
    }
    let path = if folder || is_note(Path::new(name)) { dir.join(name) } else { dir.join(format!("{name}.{}", NOTE_EXTS[0])) };
    if path.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "an entry of that name is there already"));
    }
    if folder {
        fs::create_dir_all(&path)?;
    } else {
        fs::create_dir_all(dir)?;
        write(&path, "")?;
    }
    Ok(path)
}

/// A new folder in `dir`, named `name` or the first free name after it.
pub fn new_folder(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let path = free_name(dir, name, None);
    fs::create_dir_all(&path)?;
    Ok(path)
}

/// A new vault: a folder named `name` in `parent`, made along with the
/// folders above it. The name is refused when empty or with a path
/// separator; a folder of that name that is there already becomes the
/// vault as it is, so a folder synced from elsewhere is not refused, but
/// a file of that name is.
pub fn new_vault(parent: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a name is one or more characters with no slash"));
    }
    let path = parent.join(name);
    if path.is_file() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a file of that name is there already"));
    }
    fs::create_dir_all(&path)?;
    Ok(path)
}

/// A note or a folder renamed in place. A note keeps its extension; the
/// name is refused when empty, when it has a path separator, or when an
/// entry of that name is there already.
pub fn rename(path: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a name is one or more characters with no slash"));
    }
    let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent"))?;
    let target = if path.is_dir() {
        parent.join(name)
    } else {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if !name.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) => parent.join(format!("{name}.{ext}")),
            _ => parent.join(name),
        }
    };
    if target == path {
        return Ok(target);
    }
    if target.exists() && !same_file_other_case(path, &target) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "an entry of that name is there already"));
    }
    fs::rename(path, &target)?;
    Ok(target)
}

/// On a file system that folds case, "note" and "Note" are one file: a
/// rename between the two is allowed.
fn same_file_other_case(a: &Path, b: &Path) -> bool {
    a.parent() == b.parent() && a.file_name().map(|n| n.to_string_lossy().to_lowercase()) == b.file_name().map(|n| n.to_string_lossy().to_lowercase())
}

/// Put a note or a folder in the system's trash.
pub fn trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// The note a wikilink names, as Obsidian resolves one: `[[Name]]`,
/// `[[folder/Name]]`, with or without the extension, a heading or an
/// alias after `#` or `|` dropped. A bare name is looked for in the
/// link's own folder first, then anywhere in the vault, the shortest
/// path first.
pub fn resolve_link(root: &Path, from: &Path, link: &str) -> Option<PathBuf> {
    let target = link.split(['#', '|']).next().unwrap_or("").trim();
    if target.is_empty() {
        return None;
    }
    let (dir_part, stem) = match target.rsplit_once('/') {
        Some((d, s)) => (Some(d), s),
        None => (None, target),
    };
    let stem = stem.strip_suffix(".md").or_else(|| stem.strip_suffix(".markdown")).unwrap_or(stem);
    let matches = |p: &Path| title(p).eq_ignore_ascii_case(stem);
    if let Some(dir) = dir_part {
        let base = root.join(dir);
        return NOTE_EXTS.iter().map(|e| base.join(format!("{stem}.{e}"))).find(|p| p.is_file());
    }
    let here = from.parent().unwrap_or(root);
    for ext in NOTE_EXTS {
        let p = here.join(format!("{stem}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    let mut found: Vec<PathBuf> = notes(root).into_iter().filter(|p| matches(p)).collect();
    found.sort_by_key(|p| p.components().count());
    found.into_iter().next()
}

/// The vault's assets folder, at its top: every note's pictures and
/// files in one place. Dotted, so it is never a folder of the person's
/// and the tree and the watcher pass it by as they do `.obsidian`.
pub const ASSETS: &str = ".assets";

pub fn assets(root: &Path) -> PathBuf {
    root.join(ASSETS)
}

/// Every file in the assets folder, at any depth, but dotted ones, by name in the file manager's order.
pub fn asset_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut folders = vec![assets(root)];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                out.push(path);
            }
        }
    }
    let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    out.sort_by(|a, b| natural_cmp(&name(a), &name(b)));
    out
}

/// The file a link or an embed names, a note's or any other: the path as
/// written from the note's folder, then from the vault's top, then a
/// file of that name in the assets folder, then anywhere in the vault
/// outside a dotted folder, the shortest path first. A heading, a size or an alias after
/// `#`, `?` or `|` is dropped, and `%20` and its like are read. The path
/// comes back with its `..` folded away, so it can be told by where it
/// is: `../.assets/a.png` from a folder is in the assets folder.
pub fn find_file(root: Option<&Path>, from: &Path, link: &str) -> Option<PathBuf> {
    if link.contains("://") {
        return None;
    }
    let name = decoded(link.split(['#', '?', '|']).next().unwrap_or(link).trim());
    if name.is_empty() {
        return None;
    }
    let here = from.parent().map(|p| lexical(&p.join(&name)));
    let top = root.map(|v| lexical(&v.join(&name)));
    if let Some(found) = here.into_iter().chain(top).find(|p| p.is_file()) {
        return Some(found);
    }
    let root = root?;
    let bare = Path::new(&name).file_name()?.to_os_string();
    let kept = assets(root).join(&bare);
    if kept.is_file() {
        return Some(kept);
    }
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                folders.push(path);
            } else if entry.file_name() == bare {
                found.push(path);
            }
        }
    }
    found.sort_by_key(|p| p.components().count());
    found.into_iter().next()
}

/// `%20` and its like, in a link's address.
pub fn decoded(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%').then(|| s.get(i + 1..i + 3)).flatten().and_then(|h| u8::from_str_radix(h, 16).ok());
        if let Some(b) = hex {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// A name in `dir` claimed for `name`: itself, then `stem 1.ext`,
/// `stem 2.ext`…, each tried by making it empty only where nothing is,
/// so two assets coming in at once never take the same name.
fn claim(dir: &Path, name: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 0.. {
        let path = dir.join(if n == 0 { format!("{stem}{ext}") } else { format!("{stem} {n}{ext}") });
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// What `write` does into a name `claim` holds: the bytes, or the copy,
/// go to a temp file of its own and are renamed over it; on a failure
/// the claim is let go.
fn fill(path: &Path, write: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<PathBuf> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp{}", std::process::id()));
    let result = write(&tmp).and_then(|_| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(path);
    }
    result.map(|_| path.to_path_buf())
}

/// Bytes kept as an asset of the vault, a picture pasted, under
/// `name` or the first free name after it, written whole. Hands back
/// where it is.
pub fn add_asset(root: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let path = claim(&assets(root), name)?;
    fill(&path, |tmp| fs::write(tmp, bytes))
}

/// A file brought into the vault as an asset: one in the vault already
/// is used where it is; one from elsewhere is copied into the assets
/// folder, under its name or the first free name after it,
/// through a temp file and a rename. Hands back where it is.
pub fn copy_asset(root: &Path, from: &Path) -> io::Result<PathBuf> {
    if from.starts_with(root) {
        return Ok(from.to_path_buf());
    }
    let name = from.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?.to_string_lossy().to_string();
    let path = claim(&assets(root), &name)?;
    fill(&path, |tmp| fs::copy(from, tmp).map(|_| ()))
}

/// The pictures a note draws; any other file is linked to.
pub const PICTURE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg"];

pub fn is_picture(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| PICTURE_EXTS.iter().any(|p| p.eq_ignore_ascii_case(e)))
}

/// Whether a file is a PDF, by its extension.
pub fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// How a note refers to a file of the vault, in standard markdown so any
/// other app reads it: `![name](path)` for a picture or a PDF, which the
/// note shows (a PDF by its first page), `[name](path)` for any other
/// file, the path from the note's folder with each part's spaces and
/// brackets written as `%20` and their like.
pub fn reference(from: &Path, file: &Path) -> String {
    let name = file.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let name = name.replace('[', "(").replace(']', ")");
    let bang = if is_picture(file) || is_pdf(file) { "!" } else { "" };
    format!("{bang}[{name}]({})", path_from(from, file, encoded))
}

/// The path to `file` from the folder `from` is in, each part written by `part`.
fn path_from(from: &Path, file: &Path, part: fn(&str) -> String) -> String {
    let dir = from.parent().unwrap_or(from);
    let (a, b): (Vec<_>, Vec<_>) = (dir.components().collect(), file.components().collect());
    let same = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), a.len() - same).collect();
    parts.extend(b[same..].iter().map(|c| part(&c.as_os_str().to_string_lossy())));
    parts.join("/")
}

/// The changes that make a note's links to `old` name `new` instead, a
/// file renamed or moved (`relink_moves` with that one move).
pub fn relink(root: &Path, note: &Path, text: &str, old: &Path, new: &Path) -> Vec<(std::ops::Range<usize>, String)> {
    relink_moves(root, note, note, text, &[(old.to_path_buf(), new.to_path_buf())])
}

/// Where a path goes under `moves` (each a file or a folder, from and
/// to): itself, or its new place when it is one of them or under one.
pub fn moved_path(path: &Path, moves: &[(PathBuf, PathBuf)]) -> PathBuf {
    for (from, to) in moves {
        if path == from {
            return to.clone();
        }
        if let Ok(rest) = path.strip_prefix(from) {
            return to.join(rest);
        }
    }
    path.to_path_buf()
}

/// The changes, as byte ranges of `text` and what goes there, that keep
/// a note's links going where they went when files and folders move
/// (`moves`) and the note itself goes from `note` to `moved_note`: a
/// markdown link's or picture's address (in angle brackets or not), a
/// wikilink's or an embed's target (its alias and size kept), an HTML
/// `src` or `href`. Each is written as it was: a bare name stays a bare
/// name, a path stays a path from the note's folder (a wikilink's from
/// the vault's top), what came after `#` or `?` kept, a wikilink to a
/// note without its extension kept so. A link's words that were the old
/// file's name become the new one's. A link is a file's when it finds
/// that file (`find_file`, `resolve_link` for a wikilink to a note), so
/// this runs while the files are still where they were. Only a link that
/// could be one of them is looked up: one whose name is the name of
/// something moved, or any path while the note itself moves.
pub fn relink_moves(root: &Path, note: &Path, moved_note: &Path, text: &str, moves: &[(PathBuf, PathBuf)]) -> Vec<(std::ops::Range<usize>, String)> {
    use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
    use std::collections::HashSet;
    // The names of everything moved, files under a folder moved too, with
    // and without their extensions, to know a link that could be one.
    let mut names: HashSet<String> = HashSet::new();
    for (from, _) in moves {
        let mut stack = vec![from.clone()];
        while let Some(p) = stack.pop() {
            if let Some(n) = p.file_name().map(|n| n.to_string_lossy().to_string()) {
                names.insert(n.to_lowercase());
                names.insert(title(&p).to_lowercase());
            }
            if p.is_dir() {
                stack.extend(fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()));
            }
        }
    }
    let note_moves = note != moved_note;
    let stem = |p: &Path| p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    // What a link goes to, while it may be one of the moved.
    let target = |dest: &str, wiki: bool| -> Option<PathBuf> {
        let bare = dest.split(['#', '?', '|']).next().unwrap_or(dest).trim();
        if bare.is_empty() || bare.contains("://") || bare.starts_with("mailto:") {
            return None;
        }
        let last = decoded(bare.rsplit('/').next().unwrap_or(bare)).to_lowercase();
        if !names.contains(&last) && !(note_moves && bare.contains('/')) {
            return None;
        }
        find_file(Some(root), note, bare).or_else(|| if wiki { resolve_link(root, note, bare) } else { None }).map(|p| lexical(&p))
    };
    // The address that takes the old one's place, its tail kept.
    let readdress = |dest: &str, old: &Path, new: &Path, raw: bool, wiki: bool| -> Option<String> {
        let cut = dest.find(['#', '?']).unwrap_or(dest.len());
        let (path, tail) = dest.split_at(cut);
        let path = path.trim();
        let part: fn(&str) -> String = if raw || wiki { |p| p.to_string() } else { encoded };
        // A wikilink to a note may leave its extension out, and keeps it out.
        let bare_note = wiki && is_note(new) && Path::new(path).extension().is_none();
        let name_of = |p: &Path| if bare_note { title(p) } else { p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() };
        let written = if !path.contains('/') {
            if old.file_name() == new.file_name() {
                return None;
            }
            part(&name_of(new))
        } else if wiki {
            let rel = new.strip_prefix(root).unwrap_or(new);
            let mut parts: Vec<String> = rel.components().map(|c| part(&c.as_os_str().to_string_lossy())).collect();
            if let Some(last) = parts.last_mut() {
                *last = part(&name_of(new));
            }
            parts.join("/")
        } else {
            path_from(moved_note, new, part)
        };
        (written != path).then(|| format!("{written}{tail}"))
    };
    let mut edits = Vec::new();
    let options = Options::ENABLE_TABLES | Options::ENABLE_WIKILINKS | Options::ENABLE_FOOTNOTES | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS | Options::ENABLE_MATH;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { link_type, ref dest_url, .. } | Tag::Image { link_type, ref dest_url, .. }) => {
                let src = &text[range.clone()];
                let wiki = matches!(link_type, LinkType::WikiLink { .. });
                if !wiki && !matches!(link_type, LinkType::Inline) {
                    continue;
                }
                let Some(old) = target(dest_url, wiki) else { continue };
                let new = moved_path(&old, moves);
                if new == old && !note_moves {
                    continue;
                }
                if wiki {
                    // `[[target|alias]]`: the target is what follows the brackets' opening.
                    let Some(open) = src.find("[[") else { continue };
                    let from = range.start + open + 2;
                    let to = from + text[from..range.end].find(['|', ']']).unwrap_or(0);
                    if let Some(written) = readdress(&text[from..to], &old, &new, true, true) {
                        edits.push((from..to, written));
                    }
                    continue;
                }
                // `[words](dest "title")`: the address after the last `](`.
                let Some(close) = src.rfind("](") else { continue };
                let after = range.start + close + 2;
                let rest = &text[after..range.end.saturating_sub(1)];
                let (from, to, raw) = if let Some(inner) = rest.strip_prefix('<') {
                    let end = inner.find('>').unwrap_or(inner.len());
                    (after + 1, after + 1 + end, true)
                } else {
                    let end = rest.find([' ', '\t', '\n']).unwrap_or(rest.len());
                    (after, after + end, false)
                };
                if let Some(written) = readdress(&text[from..to], &old, &new, raw, false) {
                    edits.push((from..to, written));
                }
                // Words that named the file name the new one.
                let words_from = range.start + src.find('[').map_or(0, |i| i + 1);
                let words_to = range.start + close;
                let (old_stem, new_stem) = (stem(&old), stem(&new));
                if old_stem != new_stem && words_from <= words_to && text[words_from..words_to] == old_stem {
                    edits.push((words_from..words_to, new_stem));
                }
            }
            Event::Html(ref html) | Event::InlineHtml(ref html) => {
                for attr in ["src=", "href="] {
                    let mut at = 0;
                    while let Some(i) = html[at..].find(attr) {
                        let q = at + i + attr.len();
                        at = q;
                        let Some(quote) = html[q..].chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
                        let Some(end) = html[q + 1..].find(quote) else { continue };
                        let dest = &html[q + 1..q + 1 + end];
                        if let Some(old) = target(dest, false) {
                            let new = moved_path(&old, moves);
                            if new != old || note_moves {
                                if let Some(written) = readdress(dest, &old, &new, false, false) {
                                    // The event's text is the source's, at its range.
                                    let from = range.start + q + 1;
                                    edits.push((from..from + end, written));
                                }
                            }
                        }
                        at = q + 1 + end;
                    }
                }
            }
            _ => {}
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    edits.dedup_by(|a, b| a.0 == b.0);
    edits
}

/// Files and folders moved into `into` (each keeping its name), and the
/// links to them and from the notes among them with them: every note
/// but those in `open` written whole, the open ones' left for their
/// editors (`relink_moves` from what each holds, before this runs). One
/// already there, or a folder put into itself, is passed by; a name
/// taken there refuses the whole move before anything moves. Hands back
/// the moves made.
pub fn move_into(root: &Path, items: &[PathBuf], into: &Path, open: &[PathBuf]) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let moves = planned_moves(items, into)?;
    if moves.is_empty() {
        return Ok(moves);
    }
    let mut found = Vec::new();
    for note in notes(root).into_iter().filter(|n| !open.contains(n)) {
        let Ok(text) = read(&note) else { continue };
        let edits = relink_moves(root, &note, &moved_path(&note, &moves), &text, &moves);
        if !edits.is_empty() {
            found.push((note, apply_edits(&text, &edits)));
        }
    }
    for (from, to) in &moves {
        fs::rename(from, to)?;
    }
    for (note, text) in found {
        write(&moved_path(&note, &moves), &text)?;
    }
    Ok(moves)
}

/// The moves `move_into` would make: each item to `into`, but those
/// already there, a folder into itself, and an item inside another
/// moved with it; refused when a name is taken there.
pub fn planned_moves(items: &[PathBuf], into: &Path) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let mut moves = Vec::new();
    for item in items {
        if item.parent() == Some(into) || into.starts_with(item) || items.iter().any(|other| other != item && item.starts_with(other)) {
            continue;
        }
        let Some(name) = item.file_name() else { continue };
        let to = into.join(name);
        if to.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} is there already", name.to_string_lossy())));
        }
        moves.push((item.clone(), to));
    }
    Ok(moves)
}

/// `edits` made to `text`, each a byte range of it.
pub fn apply_edits(text: &str, edits: &[(std::ops::Range<usize>, String)]) -> String {
    let mut out = text.to_string();
    for (range, with) in edits.iter().rev() {
        out.replace_range(range.clone(), with);
    }
    out
}

/// A path with its `.` and `..` worked out by its words alone, as a
/// link's path from a note's folder is read, so it can be compared with
/// a path that never went up and down.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Where an asset goes by a new name: beside itself, keeping its
/// extension, refused when empty, with a slash, or taken.
pub fn asset_renamed(path: &Path, name: &str) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent"))?;
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a name is one or more characters with no slash"));
    }
    let new = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !name.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) => parent.join(format!("{name}.{ext}")),
        _ => parent.join(name),
    };
    if new != path && new.exists() && !same_file_other_case(path, &new) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "an entry of that name is there already"));
    }
    Ok(new)
}

/// An asset moved to `new` (`asset_renamed`), and the links to it in
/// every note but those in `open` written to the note's file whole. The
/// window changes an open note's through its editor, from what the
/// editor holds, before this runs (`relink` needs the old file there).
pub fn rename_asset(root: &Path, path: &Path, new: &Path, open: &[PathBuf]) -> io::Result<()> {
    if new == path {
        return Ok(());
    }
    let mut found = Vec::new();
    for note in notes(root).into_iter().filter(|n| !open.contains(n)) {
        let Ok(text) = read(&note) else { continue };
        let edits = relink(root, &note, &text, path, new);
        if !edits.is_empty() {
            found.push((note, apply_edits(&text, &edits)));
        }
    }
    fs::rename(path, new)?;
    for (note, text) in found {
        write(&note, &text)?;
    }
    Ok(())
}

/// A part of a link's path with what markdown would misread written as
/// `%20` and its like; every other character stays as it is.
fn encoded(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for c in part.chars() {
        match c {
            ' ' | '(' | ')' | '<' | '>' | '[' | ']' | '%' | '#' | '?' | '|' | '\\' | '`' => out.push_str(&format!("%{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// The watcher `watch` hands back; it watches for as long as it is kept.
pub type Watcher = notify::RecommendedWatcher;

/// The vault watched: `on_change` is called, on the watcher's own
/// thread, with the paths that changed. The watcher is handed back and
/// watches for as long as it is kept.
pub fn watch(root: &Path, on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Result<Watcher, String> {
    use notify::Watcher as _;
    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, notify::EventKind::Access(_)) {
                return;
            }
            on_change(ev.paths);
        }
    })
    .map_err(|e| e.to_string())?;
    watcher.watch(root, notify::RecursiveMode::Recursive).map_err(|e| e.to_string())?;
    Ok(watcher)
}
