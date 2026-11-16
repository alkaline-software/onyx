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
    /// Counts the changes of `issues`, for what draws them elsewhere.
    pub(super) issues_gen: u64,
    issues_for: String,
    serial: u64,
    at_caret: bool,
    /// What each block last held wrong, by its words.
    runs: Arc<Mutex<HashMap<u64, Vec<Issue>>>>,
    /// What the system's checker last found in each block, by its words,
    /// for a language other than English: only blocks not seen before go
    /// to it, where the whole note went at every pause.
    sys_runs: Arc<Mutex<HashMap<u64, Vec<Issue>>>>,
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

/// The system's spelling of the note's blocks, for a language other than
/// English: those not seen before go to the checker together, a blank line
/// between each, and what it finds is kept by each block's words; the rest
/// are answered from what was kept.
async fn system_spelling(prose: &[crate::markdown::Prose], kept: &Arc<Mutex<HashMap<u64, Vec<Issue>>>>, language: &str) -> Vec<Issue> {
    let mut found = Vec::new();
    let mut fresh: Vec<(usize, u64)> = Vec::new();
    let mut joined = String::new();
    let mut at = Vec::new();
    {
        let known = kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for (ix, run) in prose.iter().enumerate() {
            let key = key_of(&run.text, false, false);
            if known.contains_key(&key) || fresh.iter().any(|(_, k)| *k == key) {
                continue;
            }
            fresh.push((ix, key));
            at.push(joined.len());
            joined.push_str(&run.text);
            joined.push_str("\n\n");
        }
    }
    let read = if joined.trim().is_empty() { Vec::new() } else { crate::sys::spelling(&joined, language).await };
    let mut known = kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut now: HashMap<u64, Vec<Issue>> = HashMap::new();
    for (n, (ix, key)) in fresh.iter().enumerate() {
        let (from, len) = (at[n], prose[*ix].text.len());
        let of_run = read.iter().filter(|i| i.range.start >= from && i.range.end <= from + len).map(|i| {
            let mut i = i.clone();
            i.range = i.range.start - from..i.range.end - from;
            i
        });
        now.insert(*key, of_run.collect());
    }
    for run in prose {
        let key = key_of(&run.text, false, false);
        let issues = now.get(&key).or_else(|| known.get(&key)).cloned().unwrap_or_default();
        found.extend(issues.iter().cloned().map(|mut i| {
            i.range = i.range.start + run.start..i.range.end + run.start;
            i
        }));
        now.entry(key).or_insert(issues);
    }
    *known = now;
    found
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
            w.sys_runs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
        }
        self.check_afresh(cx);
    }

    /// A text the marks were not made on: they go until it is read.
    pub(super) fn check_afresh(&mut self, cx: &mut Context<Self>) {
        self.writing.issues.clear();
        self.writing.issues_gen += 1;
        self.writing.issues_for = self.doc.text();
        self.check_soon(cx);
    }

    /// One change of the text: the marks move with the words.
    pub(super) fn text_changed(&mut self, cx: &mut Context<Self>) {
        let new = self.doc.text();
        let w = &mut self.writing;
        if w.language.is_some() {
            w.issues = check::carry(&w.issues_for, &new, &w.issues);
            w.issues_gen += 1;
        }
        w.issues_for = new;
        self.check_soon(cx);
    }

    /// The check a moment after the last change: the text and its reading
    /// are taken when the moment is up, not at each key, which copied the
    /// whole note twice a key to throw the copies away at the next.
    fn check_soon(&mut self, cx: &mut Context<Self>) {
        let w = &mut self.writing;
        w.serial += 1;
        let serial = w.serial;
        w.at_caret = false;
        let text = &w.issues_for;
        if w.language.is_none() || text.trim().is_empty() || text.len() > CHECK_MOST {
            w.issues.clear();
            w.issues_gen += 1;
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHECK_AFTER).await;
            let same = |e: &Editor| e.writing.serial == serial;
            let Some((language, text, analysis, runs, sys_runs)) = this
                .read_with(cx, |e, _| same(e).then(|| (e.writing.language.clone(), e.writing.issues_for.clone(), e.analysis.clone(), e.writing.runs.clone(), e.writing.sys_runs.clone())))
                .ok()
                .flatten()
            else {
                return;
            };
            let Some(language) = language else { return };
            let written = text.clone();
            let english = check::is_english(&language);
            let british = language == "en-GB";
            let (mask, found, prose) = cx
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
                    drop(known);
                    (mask, found, prose)
                })
                .await;
            // A rule that read across something blanked read words that
            // are not there: "and `x` and" is no repeated word.
            let mut found: Vec<Issue> = found.into_iter().filter(|i| same_text(&written, &mask, &i.range)).collect();
            if !english {
                found.extend(system_spelling(&prose, &sys_runs, &language).await);
            }
            let shown = this.update(cx, |e, cx| {
                if !same(e) {
                    return false;
                }
                let words = e.writing.words.clone();
                e.writing.issues = check::keep(&mask, found, &words.all_learned(), &words.ignored);
                e.writing.issues_gen += 1;
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
    pub(crate) fn issue_at(&self, offset: usize) -> Option<MenuIssue> {
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
