//! What the windows do to a vault's files, kept so ⌘Z takes it back and
//! ⌘⇧Z does it again (`onyx_core::fsops`, Emaki's): a removal, a move, a
//! rename. One journal for the app, so either window's key takes back
//! the last thing done, whichever window did it.
//!
//! A file removed is held in `~/.onyx/state/held`, out of the vault, and
//! reaches the system's trash once neither key can bring it back: when
//! something new is done after a ⌘Z, when it is the hundred-and-first
//! thing done, when the vault changes, at quit, and at the next launch
//! for what a crash left. The Trash is not read back from: an app is not
//! let into it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{App, Global};
use onyx_core::fsops::Journal;
pub(crate) use onyx_core::fsops::{Kind, Op};
use onyx_core::{fsops, paths, vault};

use crate::folder_look;

/// The journal, and the looks of the folders it holds, by the place
/// each is held in, to give back with the folder.
struct Files {
    journal: Journal,
    looks: HashMap<PathBuf, folder_look::Taken>,
}

impl Global for Files {}

fn files(cx: &mut App) -> Option<&mut Files> {
    if cx.has_global::<Files>() { Some(cx.global_mut::<Files>()) } else { None }
}

fn hold() -> PathBuf {
    paths::state_dir().join("held")
}

/// A probe's copy says what would go to the trash and leaves it held:
/// the Trash is the person's.
fn probing() -> bool {
    std::env::var_os("ONYX_GO").is_some()
}

/// What is held and can no longer be reached goes to the system's trash,
/// and the places it stood in are removed.
fn flush(held: Vec<PathBuf>, cx: &mut App) {
    if held.is_empty() {
        return;
    }
    if let Some(files) = files(cx) {
        files.looks.retain(|at, _| !held.contains(at));
    }
    for path in &held {
        if probing() {
            eprintln!("trash {}", path.display());
        } else if let Err(e) = vault::trash(path) {
            eprintln!("onyx: {} stays held: {e}", path.display());
        }
    }
    Journal::tidy(&hold());
}

/// The journal the app starts with. What the launch before left held, a
/// quit or a crash ago, goes to the trash first: nothing knows any more
/// where it came from.
pub(crate) fn start(cx: &mut App) {
    let hold = hold();
    cx.set_global(Files { journal: Journal::new(hold.clone()), looks: HashMap::new() });
    flush(Journal::leftovers(&hold), cx);
    Journal::tidy(&hold);
    cx.on_app_quit(|cx| {
        forget(cx);
        async {}
    })
    .detach();
}

/// Everything done is forgotten, and what is held goes to the trash: at
/// quit, and when the main window's vault changes.
pub(crate) fn forget(cx: &mut App) {
    let Some(files) = files(cx) else { return };
    let held = files.journal.close();
    flush(held, cx);
}

/// A thing done to the files is written down.
pub(crate) fn record(kind: Kind, moves: Vec<(PathBuf, PathBuf)>, cx: &mut App) {
    if moves.is_empty() {
        return;
    }
    let Some(files) = files(cx) else { return };
    let out = files.journal.record(Op { kind, moves });
    flush(out, cx);
}

/// Files and folders out of their vault, held so ⌘Z puts them back
/// exactly, a folder's icon and color with it. One the holding folder
/// cannot take (another volume) goes to the trash at once. Whatever has
/// them open lets go of them first. Hands back how many went straight to
/// the trash, and why one stayed where it was.
pub(crate) fn remove(paths: &[PathBuf], cx: &mut App) -> (usize, Option<String>) {
    let (mut moves, mut straight, mut failed) = (Vec::new(), 0, None);
    for path in paths {
        let Some(slot) = files(cx).map(|f| f.journal.slot(path)) else { continue };
        let looks = folder_look::take(path, cx);
        match fsops::move_path(path, &slot) {
            Ok(()) => {
                if let Some(files) = files(cx) {
                    files.looks.insert(slot.clone(), looks);
                }
                moves.push((path.clone(), slot));
            }
            Err(_) if probing() => eprintln!("trash {}", path.display()),
            Err(_) => match vault::trash(path) {
                Ok(()) => straight += 1,
                Err(e) => {
                    folder_look::put_back(looks, cx);
                    failed = Some(e);
                }
            },
        }
    }
    record(Kind::Removed, moves, cx);
    (straight, failed)
}

/// The step ⌘Z would take, or with `again` ⌘⇧Z, as the moves it would
/// run: looked at before it is taken, for what must be let go of first.
pub(crate) fn next(again: bool, cx: &App) -> Option<(Kind, Vec<(PathBuf, PathBuf)>)> {
    let op = cx.try_global::<Files>()?.journal.peek(again)?;
    Some((op.kind, op.run_as(again)))
}

/// Whether a path is held, out of every vault.
pub(crate) fn held(path: &Path, cx: &App) -> bool {
    cx.try_global::<Files>().is_some_and(|f| f.journal.holds(path))
}

/// The step taken out of the journal, for a window to run.
pub(crate) fn take(again: bool, cx: &mut App) -> Option<Op> {
    files(cx)?.journal.take(again)
}

/// How a step taken went: run, it can be run the other way; not run, it
/// is forgotten and what it holds goes to the trash.
pub(crate) fn settle(op: Op, again: bool, ran: bool, cx: &mut App) {
    let Some(files) = files(cx) else { return };
    if ran {
        files.journal.took(op, again);
    } else {
        let held = files.journal.gave_up(&op);
        flush(held, cx);
    }
}

/// A removal's moves run, one way or the other, whole or not at all: a
/// folder's icon and color leave with it and come back with it.
pub(crate) fn run_removal(moves: &[(PathBuf, PathBuf)], cx: &mut App) -> Result<(), String> {
    let mut leaving: Vec<(PathBuf, folder_look::Taken)> = Vec::new();
    for (from, to) in moves {
        if held(to, cx) {
            leaving.push((to.clone(), folder_look::take(from, cx)));
        }
    }
    if let Err(why) = fsops::run(moves) {
        for (_, looks) in leaving {
            folder_look::put_back(looks, cx);
        }
        return Err(why);
    }
    let back: Vec<folder_look::Taken> = match files(cx) {
        Some(files) => {
            files.looks.extend(leaving);
            moves.iter().filter_map(|(from, _)| files.looks.remove(from)).collect()
        }
        None => Vec::new(),
    };
    for looks in back {
        folder_look::put_back(looks, cx);
    }
    Ok(())
}

/// A move's or a rename's step run on the disk alone, with no window's
/// editors: every note that links to what moved is written whole
/// (`vault::move_into`), and a rename is the file's alone. For the menu
/// bar note while the main window is shut.
pub(crate) fn run_on_disk(kind: Kind, moves: &[(PathBuf, PathBuf)], root: Option<&Path>) -> Result<(), String> {
    for (from, to) in moves {
        match (kind, to.parent(), root) {
            (Kind::Moved, Some(into), Some(root)) => vault::move_into(root, std::slice::from_ref(from), into, &[]).map(|_| ()).map_err(|e| e.to_string())?,
            _ => fsops::move_path(from, to)?,
        }
    }
    Ok(())
}
