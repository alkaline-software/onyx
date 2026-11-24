//! The terminal where there is none: an iPhone and an iPad, whose apps
//! start no process, so no shell. The window is written for a terminal
//! (`shell.rs`), and this stands in its place with nothing in it, so the
//! window's code is one on every system. `Shell::toggle_terminal` never
//! shows it there, and its button and its settings are left out.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, Context, EventEmitter, FocusHandle, Focusable, Hsla, IntoElement, KeyBinding, Pixels, Point, Render, SharedString, Window, div};
use gpui_component::ActiveTheme as _;
use onyx_core::config::TerminalConfig;

use crate::menu::Entry;

pub(crate) const TERM_W: f32 = 520.;
pub(crate) const TERM_LEAST: f32 = 300.;
pub(crate) const SHEET_R: f32 = 10.;

pub(crate) fn key_bindings(_: &App) -> Vec<KeyBinding> {
    Vec::new()
}

pub(crate) type TermMenu = Rc<dyn Fn(Point<Pixels>, Vec<Entry>, &mut Window, &mut App)>;

pub(crate) enum TermEvent {
    #[allow(dead_code)]
    Away,
}

pub(crate) struct TermPanel {
    focus: FocusHandle,
}

impl EventEmitter<TermEvent> for TermPanel {}

impl TermPanel {
    pub(crate) fn new(_: PathBuf, _: TerminalConfig, _: SharedString, _: TermMenu, cx: &mut Context<Self>) -> Self {
        Self { focus: cx.focus_handle() }
    }

    pub(crate) fn set_cwd(&mut self, _: PathBuf) {}

    pub(crate) fn set_look(&mut self, _: TerminalConfig, _: SharedString, _: &mut Context<Self>) {}

    pub(crate) fn set_shown(&mut self, _: bool, _: &mut Context<Self>) {}

    pub(crate) fn ground(&self, cx: &App) -> Hsla {
        cx.theme().background
    }

    pub(crate) fn menu_do(&mut self, _: &str, _: &mut Window, _: &mut Context<Self>) {}

    pub(crate) fn probe(&mut self, _: &str, _: &mut Window, _: &mut Context<Self>) {}
}

impl Focusable for TermPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TermPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
