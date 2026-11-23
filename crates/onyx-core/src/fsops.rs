//! What was done to a vault's files, kept so it can be taken back and
//! done again: a removal, a move, a rename. Emaki's, for its files panel.
//!
//! Every one of them is a list of moves, a path before and a path after.
//! A removal's path after is in the holding folder: a file removed is
//! moved there and not thrown away, so taking the removal back is the
//! same move the other way. Nothing here deletes anything. What is in
//! the holding folder and can no longer be reached by either key is
//! handed back to the caller, who puts it in the system's trash.
//!
//! The journal keeps the steps and runs none of them: a move or a rename
//! taken back has links to write again, which is the window's to do. It
//! hands a step out (`take`) and is told how it went (`took`, `gave_up`).

use std::path::{Path, PathBuf};

/// What was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Removed,
    Moved,
    Renamed,
}

/// One thing done: its kind and its moves, each from where a path was
/// before to where it was after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    pub kind: Kind,
    pub moves: Vec<(PathBuf, PathBuf)>,
}

impl Op {
    /// The moves as a step runs them: as they were made when the thing is
    /// done again, the other way and last first when it is taken back.
    pub fn run_as(&self, again: bool) -> Vec<(PathBuf, PathBuf)> {
        if again { self.moves.clone() } else { self.moves.iter().rev().map(|(before, after)| (after.clone(), before.clone())).collect() }
    }
}

/// The things done and the things taken back, newest last, and the
/// folder what is out of the vault is held in.
#[derive(Debug)]
pub struct Journal {
    hold: PathBuf,
    done: Vec<Op>,
    undone: Vec<Op>,
    next: u64,
}

/// How many things done are kept to take back.
const MOST: usize = 100;

/// Move a file or a folder to a path nothing is at. Its folder is made
/// when it is not there. Across two volumes it is not moved: a rename is
/// all this does, so nothing is ever half copied.
pub fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::symlink_metadata(from).is_err() {
        return Err(format!("{} is no longer there", name(from)));
    }
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(format!("{} is already there", name(to)));
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

/// Moves run in their order. One that fails puts back those already run,
/// so a thing is taken back whole or not at all.
pub fn run(moves: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    for (ix, (from, to)) in moves.iter().enumerate() {
        if let Err(why) = move_path(from, to) {
            for (from, to) in moves[..ix].iter().rev() {
                let _ = move_path(to, from);
            }
            return Err(why);
        }
    }
    Ok(())
}

fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string())
}

impl Journal {
    pub fn new(hold: PathBuf) -> Self {
        Journal { hold, done: Vec::new(), undone: Vec::new(), next: 0 }
    }

    /// A free place in the holding folder for something of this name.
    /// Nothing is made there until something is moved to it.
    pub fn slot(&mut self, of: &Path) -> PathBuf {
        loop {
            self.next += 1;
            let at = self.hold.join(format!("{}-{}", std::process::id(), self.next));
            if !at.exists() {
                return at.join(of.file_name().unwrap_or_default());
            }
        }
    }

    /// Whether a path is in the holding folder.
    pub fn holds(&self, path: &Path) -> bool {
        path.starts_with(&self.hold)
    }

    /// What of an op is in the holding folder now.
    fn held_by(&self, op: &Op) -> Vec<PathBuf> {
        op.moves.iter().flat_map(|(a, b)| [a, b]).filter(|p| self.holds(p) && std::fs::symlink_metadata(p).is_ok()).cloned().collect()
    }

    /// A thing has been done. What was taken back before it cannot be
    /// done again, and the oldest thing done falls off a full list: what
    /// either holds is returned, for the trash.
    pub fn record(&mut self, op: Op) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::mem::take(&mut self.undone).iter().flat_map(|old| self.held_by(old)).collect();
        self.done.push(op);
        while self.done.len() > MOST {
            let old = self.done.remove(0);
            out.extend(self.held_by(&old));
        }
        out
    }

    /// The step a key would take, left where it is: the last thing done,
    /// or with `again` the last taken back.
    pub fn peek(&self, again: bool) -> Option<&Op> {
        if again { self.undone.last() } else { self.done.last() }
    }

    /// The step handed out to be run.
    pub fn take(&mut self, again: bool) -> Option<Op> {
        if again { self.undone.pop() } else { self.done.pop() }
    }

    /// The step ran: it can now be run the other way.
    pub fn took(&mut self, op: Op, again: bool) {
        if again { self.done.push(op) } else { self.undone.push(op) }
    }

    /// The step could not be run (a name since taken, a file since gone):
    /// it is forgotten, and what it holds returned for the trash.
    pub fn gave_up(&self, op: &Op) -> Vec<PathBuf> {
        self.held_by(op)
    }

    /// Everything in a holding folder, with nothing kept of what it was:
    /// what a launch finds left by the one before, and what a quit
    /// leaves. For the trash.
    pub fn leftovers(hold: &Path) -> Vec<PathBuf> {
        let Ok(slots) = std::fs::read_dir(hold) else { return Vec::new() };
        slots.flatten().flat_map(|slot| std::fs::read_dir(slot.path()).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>()).collect()
    }

    /// Forget everything, as at a quit: what is held is returned.
    pub fn close(&mut self) -> Vec<PathBuf> {
        self.done.clear();
        self.undone.clear();
        Self::leftovers(&self.hold)
    }

    /// The holding folder's empty places removed, once what they held
    /// has gone to the trash.
    pub fn tidy(hold: &Path) {
        let Ok(slots) = std::fs::read_dir(hold) else { return };
        for slot in slots.flatten() {
            let _ = std::fs::remove_dir(slot.path());
        }
    }
}
