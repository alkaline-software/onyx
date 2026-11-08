//! Onyx's core: everything without a window. The app crate draws it;
//! the CLI (`onyx-core`) runs each part from a terminal.

pub mod attach;
pub mod check;
pub mod config;
pub mod docx;
pub mod html;
pub mod link_card;
pub mod merge;
pub mod paths;
pub mod pdf;
pub mod preview;
pub mod process;
mod reading;
pub mod rhythm;
pub mod search;
pub mod update;
pub mod vault;
pub mod web;
