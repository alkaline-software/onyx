//! Onyx's core: everything without a window. The app crate draws it;
//! the CLI (`onyx-core`) runs each part from a terminal.

pub mod attach;
pub mod check;
pub mod config;
pub mod docx;
pub mod fsops;
pub mod html;
pub mod link_card;
pub mod merge;
pub mod paths;
pub mod pdf;
pub mod preview;
pub mod process;
// No shell on an iPhone or an iPad: an app there starts no process.
#[cfg(not(target_os = "ios"))]
pub mod pty;
mod reading;
pub mod rhythm;
pub mod search;
#[cfg(not(target_os = "ios"))]
pub mod shell_marks;
pub mod shortcut;
pub mod update;
pub mod vault;
// The folders' watcher where the system has none for a tree; built for
// a Mac's tests too, which is where it is checked.
#[cfg(any(target_os = "ios", all(test, target_os = "macos")))]
mod watch;
pub mod web;
