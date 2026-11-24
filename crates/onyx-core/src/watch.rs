//! A folder watched on an iPhone and an iPad, where the system has no
//! watcher for a whole tree. One kqueue holds a descriptor for each folder,
//! never one for each file: an app there may hold a few hundred files open,
//! and a vault has thousands of notes. A folder's event says only that its
//! entries changed, so the folder is listed again and set against what it
//! held: what came, went, or has another time or length is what changed.
//!
//! A note written by a rename, as Onyx and iCloud write one, changes its
//! folder. One written in place by another app changes no folder, so
//! `Watcher::look` lists every folder again, and the window asks for that
//! when it comes back to the front, the only time another app's write can
//! have happened.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The watcher: it watches for as long as it is kept.
pub struct Watcher {
    /// The pipe the thread wakes on: a `STOP` ends it, a `LOOK` has it
    /// list every folder again.
    wake: RawFd,
    thread: Option<std::thread::JoinHandle<()>>,
}

const STOP: u8 = 0;
const LOOK: u8 = 1;

impl Watcher {
    /// Every folder listed again, for a change that moved no entry.
    pub fn look(&self) {
        say(self.wake, LOOK);
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        say(self.wake, STOP);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: the pipe's writing end is this watcher's alone.
        unsafe { libc::close(self.wake) };
    }
}

fn say(wake: RawFd, what: u8) {
    // SAFETY: one byte from a live local into a descriptor this module opened.
    unsafe { libc::write(wake, (&what as *const u8).cast(), 1) };
}

/// What a folder's entry was when it was last listed.
#[derive(PartialEq, Clone, Copy)]
struct Stamp {
    dir: bool,
    modified: Option<SystemTime>,
    len: u64,
}

struct Folder {
    path: PathBuf,
    entries: HashMap<OsString, Stamp>,
}

/// Whether an entry is watched and listed: a vault's own folders are not
/// (`.obsidian`, `.git`, `.trash`), but its assets are.
fn listed(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    !name.starts_with('.') || name == ".assets"
}

fn list(dir: &Path) -> HashMap<OsString, Stamp> {
    let Ok(read) = std::fs::read_dir(dir) else { return HashMap::new() };
    read.flatten()
        .filter(|e| listed(&e.file_name()))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            // A folder's own time changes with its entries, which its own
            // descriptor says: here it is only that it is a folder.
            let stamp = if meta.is_dir() { Stamp { dir: true, modified: None, len: 0 } } else { Stamp { dir: false, modified: meta.modified().ok(), len: meta.len() } };
            Some((e.file_name(), stamp))
        })
        .collect()
}

struct Watching {
    queue: RawFd,
    folders: HashMap<RawFd, Folder>,
    deep: bool,
}

impl Watching {
    /// A folder taken in, and with `deep` the folders in it.
    fn add(&mut self, path: &Path) {
        let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else { return };
        // SAFETY: a NUL-ended path; `O_EVTONLY` asks for events alone.
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_EVTONLY) };
        if fd < 0 {
            return;
        }
        let change = libc::kevent {
            ident: fd as usize,
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: libc::NOTE_WRITE | libc::NOTE_EXTEND | libc::NOTE_DELETE | libc::NOTE_RENAME,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: one change read from a live local, no events asked for.
        if unsafe { libc::kevent(self.queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
            // SAFETY: the descriptor opened above.
            unsafe { libc::close(fd) };
            return;
        }
        let entries = list(path);
        let inside: Vec<PathBuf> = if self.deep { entries.iter().filter(|(_, s)| s.dir).map(|(name, _)| path.join(name)).collect() } else { Vec::new() };
        self.folders.insert(fd, Folder { path: path.to_path_buf(), entries });
        for dir in inside {
            self.add(&dir);
        }
    }

    /// A folder gone or moved, and every folder that was in it, let go.
    fn forget(&mut self, path: &Path) {
        let gone: Vec<RawFd> = self.folders.iter().filter(|(_, f)| f.path.starts_with(path)).map(|(fd, _)| *fd).collect();
        for fd in gone {
            self.folders.remove(&fd);
            // SAFETY: a descriptor `add` opened; closing it takes its event off the queue.
            unsafe { libc::close(fd) };
        }
    }

    /// A folder listed again: what changed in it, with the folders that
    /// came taken in and those that went let go.
    fn changed(&mut self, fd: RawFd, out: &mut Vec<PathBuf>) {
        let Some(folder) = self.folders.get_mut(&fd) else { return };
        let path = folder.path.clone();
        if !path.is_dir() {
            out.push(path.clone());
            self.forget(&path);
            return;
        }
        let now = list(&path);
        let before = std::mem::replace(&mut folder.entries, now.clone());
        let (mut came, mut went) = (Vec::new(), Vec::new());
        for (name, stamp) in &now {
            match before.get(name) {
                Some(old) if old == stamp => {}
                Some(old) => {
                    out.push(path.join(name));
                    if old.dir != stamp.dir {
                        (if stamp.dir { &mut came } else { &mut went }).push(path.join(name));
                    }
                }
                None => {
                    out.push(path.join(name));
                    if stamp.dir {
                        came.push(path.join(name));
                    }
                }
            }
        }
        for (name, old) in &before {
            if !now.contains_key(name) {
                out.push(path.join(name));
                if old.dir {
                    went.push(path.join(name));
                }
            }
        }
        if self.deep {
            for dir in went {
                self.forget(&dir);
            }
            for dir in came {
                self.add(&dir);
            }
        }
    }
}

/// `root` watched, with `deep` every folder in it too: `on_change` is
/// called on the watcher's own thread with the paths that changed.
pub fn watch(root: &Path, deep: bool, on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Result<Watcher, String> {
    let mut ends = [0 as RawFd; 2];
    // SAFETY: two descriptors written into a live array of two.
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let (heard, wake) = (ends[0], ends[1]);
    // SAFETY: no arguments.
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        let e = std::io::Error::last_os_error().to_string();
        // SAFETY: the pipe opened above.
        unsafe { (libc::close(heard), libc::close(wake)) };
        return Err(e);
    }
    let waking = libc::kevent { ident: heard as usize, filter: libc::EVFILT_READ, flags: libc::EV_ADD, fflags: 0, data: 0, udata: std::ptr::null_mut() };
    // SAFETY: one change read from a live local.
    unsafe { libc::kevent(queue, &waking, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    let root = root.to_path_buf();
    let thread = std::thread::Builder::new()
        .name("vault watcher".into())
        .spawn(move || {
            let mut watching = Watching { queue, folders: HashMap::new(), deep };
            watching.add(&root);
            let empty = libc::kevent { ident: 0, filter: 0, flags: 0, fflags: 0, data: 0, udata: std::ptr::null_mut() };
            let mut events = [empty; 32];
            'watching: loop {
                // SAFETY: room for 32 events in a live array.
                let n = unsafe { libc::kevent(queue, std::ptr::null(), 0, events.as_mut_ptr(), events.len() as i32, std::ptr::null()) };
                if n < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    break;
                }
                let mut out = Vec::new();
                for event in &events[..n as usize] {
                    let fd = event.ident as RawFd;
                    if fd != heard {
                        watching.changed(fd, &mut out);
                        continue;
                    }
                    let mut what = [0u8; 16];
                    // SAFETY: at most 16 bytes into a live array of 16.
                    let read = unsafe { libc::read(heard, what.as_mut_ptr().cast(), what.len()) };
                    if read <= 0 || what[..read as usize].contains(&STOP) {
                        break 'watching;
                    }
                    let every: Vec<RawFd> = watching.folders.keys().copied().collect();
                    for fd in every {
                        watching.changed(fd, &mut out);
                    }
                }
                if !out.is_empty() {
                    out.sort();
                    out.dedup();
                    on_change(out);
                }
            }
            let every: Vec<RawFd> = watching.folders.keys().copied().collect();
            // SAFETY: the descriptors this thread opened, and its ends of the pipe and the queue.
            unsafe {
                for fd in every {
                    libc::close(fd);
                }
                libc::close(heard);
                libc::close(queue);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Watcher { wake, thread: Some(thread) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn heard(rx: &mpsc::Receiver<Vec<PathBuf>>) -> Vec<PathBuf> {
        let mut all = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
        while let Ok(more) = rx.recv_timeout(Duration::from_millis(200)) {
            all.extend(more);
        }
        all
    }

    #[test]
    fn a_folder_says_what_came_changed_and_went() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("Ideas")).unwrap();
        std::fs::write(root.join("Ideas/Plan.md"), "one").unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = watch(&root, true, move |paths| {
            let _ = tx.send(paths);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));

        // A note that comes, at the root and in a folder.
        std::fs::write(root.join("New.md"), "new").unwrap();
        assert!(heard(&rx).contains(&root.join("New.md")));

        // A note written again by a rename, as Onyx writes one.
        std::fs::write(root.join("Ideas/.tmp"), "two, longer").unwrap();
        std::fs::rename(root.join("Ideas/.tmp"), root.join("Ideas/Plan.md")).unwrap();
        assert!(heard(&rx).contains(&root.join("Ideas/Plan.md")));

        // A folder that comes is watched too.
        std::fs::create_dir(root.join("Later")).unwrap();
        assert!(heard(&rx).contains(&root.join("Later")));
        std::fs::write(root.join("Later/In.md"), "in").unwrap();
        assert!(heard(&rx).contains(&root.join("Later/In.md")));

        // A folder renamed: the old name went, the new one came, and a
        // note in it is still heard at its new place.
        std::fs::rename(root.join("Ideas"), root.join("Thoughts")).unwrap();
        let moved = heard(&rx);
        assert!(moved.contains(&root.join("Ideas")) && moved.contains(&root.join("Thoughts")));
        std::fs::remove_file(root.join("Thoughts/Plan.md")).unwrap();
        assert!(heard(&rx).contains(&root.join("Thoughts/Plan.md")));

        // A vault's own folders are not listed.
        std::fs::create_dir(root.join(".obsidian")).unwrap();
        std::fs::write(root.join(".obsidian/workspace.json"), "{}").unwrap();
        assert!(heard(&rx).is_empty());

        // A note written in place changes no folder: `look` finds it.
        use std::io::Write as _;
        std::fs::OpenOptions::new().append(true).open(root.join("New.md")).unwrap().write_all(b" and more").unwrap();
        watcher.look();
        assert!(heard(&rx).contains(&root.join("New.md")));
    }
}
