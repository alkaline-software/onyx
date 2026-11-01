//! Onyx's core: everything without a window. The app crate draws it;
//! the CLI (`onyx-core`) runs each part from a terminal.

pub mod check;
pub mod config;
pub mod html;
pub mod paths;
pub mod pdf;
pub mod process;
pub mod rhythm;
pub mod search;
pub mod update;
pub mod vault;
