//! A note's spelling and grammar: `onyx_core::check` run on what of the
//! note is prose (`markdown::prose`), a block at a time, off the window's
//! thread, once the typing has paused. A block is read again only when it
//! changed, so a long note costs what the paragraph being typed costs.
//!
//! The marks move with the words until the text is read again
//! (`check::carry`), and a misspelling at the caret waits a little longer
//! than the rest, so a word being typed is not marked as it is typed.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::*;
use onyx_core::check::{self, Issue, Kind};

use super::Editor;
use crate::lines::{Doc, Sel};

const CHECK_AFTER: Duration = Duration::from_millis(300);
const CHECK_WORD_AFTER: Duration = Duration::from_millis(1200);
/// Past this many bytes a note is not checked.
const CHECK_MOST: usize = 2_000_000;

/// The words the person has settled: those taught and kept, the grammar
/// ignored and kept, and the spellings ignored until the app is quit.
#[derive(Clone, Debug, Default)]
pub struct Words {
    pub learned: HashSet<String>,
    pub ignored: HashSet<String>,
    pub passed: HashSet<String>,
}

impl Words {
    pub fn load() -> Self {
        Self { learned: check::learned(), ignored: check::ignored(), passed: HashSet::new() }
    }

    fn all_learned(&self) -> HashSet<String> {
        self.learned.union(&self.passed).cloned().collect()
    }
}

/// What a right click on a mark needs.
#[derive(Clone, Debug)]
pub struct MenuIssue {
    pub range: Range<usize>,
    pub words: String,
    pub fixes: Vec<String>,
    pub spelling: bool,
    pub rule: String,
}

#[derive(Default)]
pub(super) struct Writing {
    language: Option<String>,
    words: Rc<Words>,
    issues: Vec<Issue>,
    issues_for: String,
    serial: u64,
    at_caret: bool,
    /// What each block last held wrong, by its words.
    runs: Arc<Mutex<HashMap<u64, Vec<Issue>>>>,
}

impl Writing {
    pub fn shown(&self, _doc: &Doc) -> &[Issue] {
        &self.issues
    }

    /// The offset of a word still being typed, whose mark waits.
    pub fn quiet(&self, doc: &Doc, sel: Sel) -> Option<usize> {
        (!self.at_caret && sel.is_caret()).then(|| doc.offset_of(sel.head))
    }
}

/// Whether a run of bytes reads the same in both texts.
fn same_text(a: &str, b: &str, r: &Range<usize>) -> bool {
    a.as_bytes().get(r.clone()).is_some_and(|x| b.as_bytes().get(r.clone()) == Some(x))
}

fn key_of(words: &str, british: bool, sentences: bool) -> u64 {
    let mut h = DefaultHasher::new();
    words.hash(&mut h);
    british.hash(&mut h);
    sentences.hash(&mut h);
    h.finish()
}

impl Editor {
    /// The check on in a language, or off; and the words settled.
    pub fn set_writing(&mut self, language: Option<String>, words: Rc<Words>, cx: &mut Context<Self>) {
        let w = &mut self.writing;
        let same = w.language == language;
        w.language = language;
        w.words = words;
        if !same {
            w.runs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
        }
        self.check_afresh(cx);
    }

    /// A text the marks were not made on: they go until it is read.
    pub(super) fn check_afresh(&mut self, cx: &mut Context<Self>) {
        self.writing.issues.clear();
        self.writing.issues_for = self.doc.text();
        self.check_soon(cx);
    }

    /// One change of the text: the marks move with the words.
    pub(super) fn text_changed(&mut self, cx: &mut Context<Self>) {
        let new = self.doc.text();
        let w = &mut self.writing;
        if w.language.is_some() {
            w.issues = check::carry(&w.issues_for, &new, &w.issues);
        }
        w.issues_for = new;
        self.check_soon(cx);
    }

    fn check_soon(&mut self, cx: &mut Context<Self>) {
        let w = &mut self.writing;
        w.serial += 1;
        let serial = w.serial;
        w.at_caret = false;
        let text = w.issues_for.clone();
        let Some(language) = w.language.clone().filter(|_| !text.trim().is_empty() && text.len() <= CHECK_MOST) else {
            w.issues.clear();
            return;
        };
        let (analysis, runs) = (self.analysis.clone(), w.runs.clone());
        let written = text.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHECK_AFTER).await;
            let same = |e: &Editor| e.writing.serial == serial;
            if !this.read_with(cx, |e, _| same(e)).unwrap_or(false) {
                return;
            }
            let english = check::is_english(&language);
            let british = language == "en-GB";
            let (mask, found) = cx
                .background_spawn(async move {
                    let (mask, prose) = crate::markdown::prose(&text, &analysis);
                    let mut found = Vec::new();
                    let mut kept = HashMap::new();
                    let mut known = runs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    for run in &prose {
                        let key = key_of(&run.text, british, run.sentences);
                        let held = known.get(&key).cloned();
                        let issues = held.unwrap_or_else(|| {
                            let mut issues = if english { check::english(&run.text, british) } else { Vec::new() };
                            // Read as written: a sentence after `code` does
                            // not start where the code was blanked.
                            if let Some(words) = text.get(run.start..run.start + run.text.len()).filter(|_| run.sentences) {
                                issues.extend(check::capitals(words));
                            }
                            issues
                        });
                        found.extend(issues.iter().cloned().map(|mut i| {
                            i.range = i.range.start + run.start..i.range.end + run.start;
                            i
                        }));
                        kept.insert(key, issues);
                    }
                    *known = kept;
                    (mask, found)
                })
                .await;
            // A rule that read across something blanked read words that
            // are not there: "and `x` and" is no repeated word.
            let mut found: Vec<Issue> = found.into_iter().filter(|i| same_text(&written, &mask, &i.range)).collect();
            if !english {
                found.extend(crate::sys::spelling(&mask, &language).await);
            }
            let shown = this.update(cx, |e, cx| {
                if !same(e) {
                    return false;
                }
                let words = e.writing.words.clone();
                e.writing.issues = check::keep(&mask, found, &words.all_learned(), &words.ignored);
                cx.notify();
                true
            });
            if !shown.unwrap_or(false) {
                return;
            }
            cx.background_executor().timer(CHECK_WORD_AFTER).await;
            let _ = this.update(cx, |e, cx| {
                if same(e) {
                    e.writing.at_caret = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The mark at a place, as its menu needs it.
    pub(super) fn issue_at(&self, offset: usize) -> Option<MenuIssue> {
        if self.locked() {
            return None;
        }
        let i = self.writing.issues.iter().find(|i| i.range.start <= offset && offset <= i.range.end)?;
        let words = self.writing.issues_for.get(i.range.clone())?.to_string();
        let spelling = i.kind == Kind::Spelling;
        let asked = spelling && i.fixes.is_empty();
        let fixes = match self.writing.language.as_deref() {
            Some(language) if asked && !check::is_english(language) => crate::sys::spelling_fixes(&words, language),
            _ => i.fixes.clone(),
        };
        Some(MenuIssue { range: i.range.clone(), words, fixes, spelling, rule: i.rule.clone() })
    }

    /// A correction where the marked words were, one step of undo. The
    /// range was read when the menu opened: it must still be whole
    /// characters of the text there is now.
    pub fn fix_issue(&mut self, range: Range<usize>, with: &str, cx: &mut Context<Self>) {
        if self.locked() || self.doc.text().get(range.clone()).is_none() {
            return;
        }
        let (a, b) = (self.doc.pos_of(range.start), self.doc.pos_of(range.end));
        self.doc.end_step();
        self.replace_range(a, b, with, cx);
        self.doc.end_step();
    }
}
