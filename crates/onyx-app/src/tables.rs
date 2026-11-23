//! A table's lines changed as a whole: a row or a column in, out or
//! moved, an alignment, a sort, the columns' widths. Pure text, so what
//! it does is tested with no window; the editor makes each change one
//! step of undo (`Editor::table_do`).
//!
//! Each row keeps its cells as written, spaces and all, so a table laid
//! out by hand in the file stays as it was laid, and only what a change
//! touches changes.
//!
//! A table's widths are kept in the file, in its separator row: each
//! column's share of the room in hundredths, as dashes. Adding up to 100
//! the table spans the room, and Pandoc reads the same dashes the same
//! way; less, it stands narrower (`rhythm::widths_of`). Either way it is
//! a legal table to every other reader.

use std::cmp::Ordering;

use crate::lines::quote_line;
use crate::markdown::Align;

/// A column's least share, in dashes: fewer is not a separator to some
/// readers, nor read as a width (`rhythm::widths_of`).
const LEAST_SHARE: u32 = onyx_core::rhythm::LEAST_WIDTH;

/// Where a row begins in its line: past a quote's marks and the indent.
pub fn body_start(line: &str) -> usize {
    let (_, at) = quote_line(line).unwrap_or((0, 0));
    at + (line[at..].len() - line[at..].trim_start().len())
}

/// A separator row's widths, when they say a person set them.
pub use onyx_core::rhythm::widths_of;

/// What stands between a line's bars, spaces and all.
fn segments(row: &str) -> Vec<String> {
    let t = row.trim_end();
    let t = t.strip_prefix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cell.push(c);
                if let Some(next) = chars.next() {
                    cell.push(next);
                }
            }
            '|' => out.push(std::mem::take(&mut cell)),
            _ => cell.push(c),
        }
    }
    if !cell.is_empty() {
        out.push(cell);
    }
    out
}

/// A table's lines, taken apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// What stands before the first bar on each line: a quote's marks, an indent.
    lead: Vec<String>,
    /// The rows, the head first, each as many cells as there are columns.
    rows: Vec<Vec<String>>,
    separator: Vec<String>,
}

impl Sheet {
    /// `lines`: the head, the separator, then the rest.
    pub fn read(lines: &[String]) -> Sheet {
        let split = |l: &String| {
            let at = body_start(l);
            (l[..at].to_string(), segments(&l[at..]))
        };
        let mut lead = Vec::new();
        let mut rows = Vec::new();
        let mut separator = Vec::new();
        for (ix, line) in lines.iter().enumerate() {
            let (before, cells) = split(line);
            if ix == 1 {
                separator = cells;
            } else {
                lead.push(before);
                rows.push(cells);
            }
        }
        let columns = separator.len().max(1);
        if separator.is_empty() {
            separator.push(" --- ".to_string());
        }
        for row in &mut rows {
            row.resize(columns, "  ".to_string());
        }
        Sheet { lead, rows, separator }
    }

    pub fn write(&self) -> Vec<String> {
        let lead = |ix: usize| self.lead.get(ix).or(self.lead.first()).map_or("", String::as_str);
        let line = |lead: &str, cells: &[String]| format!("{lead}|{}|", cells.join("|"));
        let mut out = Vec::with_capacity(self.rows.len() + 1);
        for (ix, row) in self.rows.iter().enumerate() {
            out.push(line(lead(ix), row));
            if ix == 0 {
                out.push(line(lead(0), &self.separator));
            }
        }
        out
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    pub fn columns(&self) -> usize {
        self.separator.len()
    }

    /// An empty row, standing at `at`.
    pub fn insert_row(&mut self, at: usize) {
        let at = at.min(self.rows.len());
        self.rows.insert(at, vec!["  ".to_string(); self.columns()]);
        let lead = self.lead.first().cloned().unwrap_or_default();
        self.lead.insert(at.min(self.lead.len()), lead);
    }

    pub fn duplicate_row(&mut self, at: usize) {
        if let Some(row) = self.rows.get(at).cloned() {
            self.rows.insert(at + 1, row);
            self.lead.insert((at + 1).min(self.lead.len()), self.lead.get(at).cloned().unwrap_or_default());
        }
    }

    /// False for the last row of all, which leaves no table.
    pub fn delete_row(&mut self, at: usize) -> bool {
        if self.rows.len() < 2 || at >= self.rows.len() {
            return false;
        }
        self.rows.remove(at);
        if at < self.lead.len() {
            self.lead.remove(at);
        }
        true
    }

    pub fn move_row(&mut self, from: usize, to: usize) {
        if from < self.rows.len() && to < self.rows.len() {
            let row = self.rows.remove(from);
            self.rows.insert(to, row);
        }
    }

    /// An empty column, standing at `at`, as wide as the others are on
    /// the whole; past the room, every column gives way in proportion.
    pub fn insert_col(&mut self, at: usize) {
        let at = at.min(self.columns());
        let shares = self.widths();
        for row in &mut self.rows {
            row.insert(at, "  ".to_string());
        }
        self.separator.insert(at, " --- ".to_string());
        if let Some(mut s) = shares {
            let fresh = s.iter().sum::<f32>() / s.len() as f32;
            s.insert(at, fresh);
            let total: f32 = s.iter().sum();
            if total > 100. {
                s.iter_mut().for_each(|w| *w *= 100. / total);
            }
            self.put(s);
        }
    }

    /// False for the last column of all, which leaves no table.
    pub fn delete_col(&mut self, at: usize) -> bool {
        if self.columns() < 2 || at >= self.columns() {
            return false;
        }
        let shares = self.widths();
        for row in &mut self.rows {
            row.remove(at);
        }
        self.separator.remove(at);
        // The others keep their widths; the table is the narrower.
        if let Some(mut s) = shares {
            s.remove(at);
            self.put(s);
        }
        true
    }

    pub fn move_col(&mut self, from: usize, to: usize) {
        if from >= self.columns() || to >= self.columns() {
            return;
        }
        for row in &mut self.rows {
            let cell = row.remove(from);
            row.insert(to, cell);
        }
        let mark = self.separator.remove(from);
        self.separator.insert(to, mark);
    }

    pub fn align_of(&self, col: usize) -> Align {
        let Some(mark) = self.separator.get(col).map(|m| m.trim()) else { return Align::None };
        match (mark.starts_with(':'), mark.ends_with(':')) {
            (true, true) => Align::Center,
            (true, false) => Align::Left,
            (false, true) => Align::Right,
            _ => Align::None,
        }
    }

    pub fn align(&mut self, col: usize, align: Align) {
        let Some(mark) = self.separator.get_mut(col) else { return };
        let dashes = mark.chars().filter(|c| *c == '-').count().max(1);
        *mark = dressed(dashes, &align);
    }

    /// The widths a person set, each column's share of the room in
    /// hundredths, as the editor reads them.
    pub fn widths(&self) -> Option<Vec<f32>> {
        let lines = self.write();
        let rows: Vec<&str> = lines.iter().enumerate().filter(|(i, _)| *i != 1).map(|(_, l)| l.as_str()).collect();
        widths_of(lines.get(1)?, &rows)
    }

    /// The columns' widths in points, in `room`: each written as its
    /// share of the room.
    pub fn set_widths(&mut self, widths: &[f32], room: f32) {
        if widths.len() != self.columns() || room <= 0. {
            return;
        }
        self.put(widths.iter().map(|w| w / room * 100.).collect());
    }

    /// Every column as wide as its words ask again: the dashes a table is
    /// typed with.
    pub fn clear_widths(&mut self) {
        for col in 0..self.columns() {
            let align = self.align_of(col);
            self.separator[col] = dressed(3, &align);
        }
    }

    /// Shares in hundredths as dashes. A table within a hundredth of the
    /// room is written spanning it, the shares adding up to exactly 100,
    /// the rounding put right on the widest. A narrower one keeps each
    /// column's own share, rounded up and none under `LEAST_SHARE`; where they all came
    /// out the same, the last takes a hundredth more, since one count of
    /// dashes in every column is how a table is typed and says nothing.
    fn put(&mut self, shares: Vec<f32>) {
        if shares.len() != self.columns() || shares.is_empty() {
            return;
        }
        let near: f32 = shares.iter().sum();
        // Up to the next hundredth below the room, so no column comes out
        // narrower than it was laid and breaks a word it held.
        let mut dashes: Vec<u32> = shares.iter().map(|w| (if near >= 99. { w.max(0.).round() } else { (w.max(0.) - 0.01).ceil() }) as u32).map(|d| d.max(LEAST_SHARE)).collect();
        if near >= 99. || dashes.iter().sum::<u32>() >= 100 {
            while dashes.iter().sum::<u32>() != 100 {
                let widest = dashes.iter().enumerate().max_by_key(|(_, d)| **d).map_or(0, |(i, _)| i);
                if dashes.iter().sum::<u32>() > 100 {
                    if dashes[widest] <= LEAST_SHARE {
                        break;
                    }
                    dashes[widest] -= 1;
                } else {
                    dashes[widest] += 1;
                }
            }
        } else if dashes.iter().all(|d| *d == dashes[0]) {
            if let Some(last) = dashes.last_mut() {
                *last += 1;
            }
        }
        self.rule(&dashes);
        // What is written reads back as widths: where the dashes happen to
        // line up with the rows, or to stand under the head as a hand's
        // would (`rhythm::widths_of`), the last column takes a hundredth
        // more.
        while self.widths().is_none() && dashes.iter().sum::<u32>() < 99 {
            if let Some(last) = dashes.last_mut() {
                *last += 1;
            }
            self.rule(&dashes);
        }
    }

    /// The separator written with those dashes, each column's alignment kept.
    fn rule(&mut self, dashes: &[u32]) {
        for (col, d) in dashes.iter().enumerate() {
            let align = self.align_of(col);
            self.separator[col] = dressed(*d as usize, &align);
        }
    }

    /// The rows under the head by a column: numbers as numbers, words
    /// whatever their marks and case, an empty cell last either way.
    pub fn sort(&mut self, col: usize, descending: bool) {
        if col >= self.columns() || self.rows.len() < 3 {
            return;
        }
        let head = self.rows.remove(0);
        let mut keyed: Vec<(String, Vec<String>)> = self.rows.drain(..).map(|r| (plain(r.get(col).map_or("", String::as_str)), r)).collect();
        keyed.sort_by(|(a, _), (b, _)| match (a.is_empty(), b.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => {
                let by = match (number(a), number(b)) {
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    _ => a.to_lowercase().cmp(&b.to_lowercase()),
                };
                if descending { by.reverse() } else { by }
            }
        });
        self.rows.push(head);
        self.rows.extend(keyed.into_iter().map(|(_, r)| r));
    }
}

/// A separator's cell: its dashes, with its colons.
fn dressed(dashes: usize, align: &Align) -> String {
    let dashes = "-".repeat(dashes);
    match align {
        Align::Center => format!(" :{dashes}: "),
        Align::Left => format!(" :{dashes} "),
        Align::Right => format!(" {dashes}: "),
        Align::None => format!(" {dashes} "),
    }
}

/// A cell's words with their marks away, for sorting: a link's words,
/// no stars, no tags.
fn plain(cell: &str) -> String {
    let mut out = String::new();
    let mut chars = cell.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' | '~' | '=' | '[' => {}
            ']' => {
                if chars.peek() == Some(&'(') {
                    for inner in chars.by_ref() {
                        if inner == ')' {
                            break;
                        }
                    }
                }
            }
            '<' => {
                for inner in chars.by_ref() {
                    if inner == '>' {
                        break;
                    }
                }
            }
            '\\' => out.extend(chars.next()),
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// The number a cell opens with, past a currency's sign, with its thousands.
fn number(cell: &str) -> Option<f64> {
    let t = cell.trim_start_matches(['$', '€', '£', '¥', '+']).replace(',', "");
    let end = t.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-'))).unwrap_or(t.len());
    let rest = t[end..].trim();
    if end == 0 || !(rest.is_empty() || rest == "%") {
        return None;
    }
    t[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(text: &str) -> Sheet {
        Sheet::read(&text.lines().map(str::to_string).collect::<Vec<_>>())
    }

    fn text(s: &Sheet) -> String {
        s.write().join("\n")
    }

    const T: &str = "| a | b |\n|---|:-:|\n| 1 | 2 |\n| 3 | 4 |";

    #[test]
    fn a_table_read_and_written_is_the_same() {
        assert_eq!(text(&sheet(T)), T);
        let wide = "| 名字   | b |\n|--------|---|\n| 费城   | 2 |";
        assert_eq!(text(&sheet(wide)), wide);
        assert_eq!(text(&sheet("> | a |\n> |---|\n> | 1 |")), "> | a |\n> |---|\n> | 1 |");
    }

    #[test]
    fn a_row_with_too_few_cells_is_filled() {
        assert_eq!(text(&sheet("| a | b |\n|---|---|\n| 1 |")), "| a | b |\n|---|---|\n| 1 |  |");
        assert_eq!(text(&sheet("a | b\n---|---\n1 | 2")), "|a | b|\n|---|---|\n|1 | 2|");
    }

    #[test]
    fn rows_go_in_out_and_move() {
        let mut s = sheet(T);
        s.insert_row(2);
        assert_eq!(text(&s), "| a | b |\n|---|:-:|\n| 1 | 2 |\n|  |  |\n| 3 | 4 |");
        assert!(s.delete_row(2));
        assert_eq!(text(&s), T);
        s.move_row(2, 1);
        assert_eq!(text(&s), "| a | b |\n|---|:-:|\n| 3 | 4 |\n| 1 | 2 |");
        s.duplicate_row(1);
        assert_eq!(s.rows(), 4);
        let mut one = sheet("| a |\n|---|");
        assert!(!one.delete_row(0));
    }

    #[test]
    fn columns_go_in_out_and_move() {
        let mut s = sheet(T);
        s.insert_col(1);
        assert_eq!(text(&s), "| a |  | b |\n|---| --- |:-:|\n| 1 |  | 2 |\n| 3 |  | 4 |");
        assert!(s.delete_col(1));
        assert_eq!(text(&s), T);
        s.move_col(1, 0);
        assert_eq!(text(&s), "| b | a |\n|:-:|---|\n| 2 | 1 |\n| 4 | 3 |");
        let mut one = sheet("| a |\n|---|\n| 1 |");
        assert!(!one.delete_col(0));
    }

    #[test]
    fn an_alignment_is_the_separators() {
        let mut s = sheet(T);
        assert_eq!((s.align_of(0), s.align_of(1)), (Align::None, Align::Center));
        s.align(0, Align::Right);
        s.align(1, Align::None);
        assert_eq!(text(&s).lines().nth(1), Some("| ---: | - |"));
        assert_eq!(s.align_of(0), Align::Right);
    }

    #[test]
    fn a_persons_widths_are_each_columns_share_of_the_room() {
        let dashes = |s: &Sheet| s.write()[1].chars().filter(|c| *c == '-').count();
        let mut s = sheet(T);
        s.align(1, Align::Center);
        // Spanning the room: dashes adding up to 100.
        s.set_widths(&[600., 200.], 800.);
        assert_eq!(dashes(&s), 100);
        assert_eq!(s.align_of(1), Align::Center);
        assert_eq!(s.widths(), Some(vec![75., 25.]));
        // Narrower: each its own share, the table the rest of the room short.
        s.set_widths(&[240., 160.], 800.);
        assert_eq!(s.widths(), Some(vec![30., 20.]));
        // Widths that would stand under the head cell for cell, as a
        // hand's rule does, still read back: the last takes a hundredth more.
        let mut u = sheet("| Skill | Invoke as |\n|---|---|\n| a | b |");
        u.set_widths(&[40., 72.], 800.);
        assert_eq!(u.widths(), Some(vec![5., 10.]));
        // A column that goes leaves the others as they were.
        let mut t = sheet("| a | b | c |\n|---|---|---|");
        t.set_widths(&[200., 160., 120.], 800.);
        assert!(t.delete_col(1));
        assert_eq!(t.widths(), Some(vec![25., 15.]));
        // One that comes past the room makes the others give way.
        t.set_widths(&[400., 400.], 800.);
        t.insert_col(1);
        assert_eq!(dashes(&t), 100);
        // Shares all the same are written so they still say widths.
        t.set_widths(&[160., 160., 160.], 800.);
        assert_eq!(t.widths().map(|w| w.iter().sum::<f32>()), Some(61.));
        t.clear_widths();
        assert_eq!(t.widths(), None);
        assert_eq!(widths_of("|---|---|", &[]), None);
        assert_eq!(widths_of("| a |", &[]), None);
    }

    #[test]
    fn a_sort_goes_by_what_a_cell_says() {
        let mut s = sheet("| n |\n|---|\n| **10** |\n|  |\n| 9 |\n| [b](http://x) |\n| A |");
        s.sort(0, false);
        assert_eq!(text(&s), "| n |\n|---|\n| 9 |\n| **10** |\n| A |\n| [b](http://x) |\n|  |");
        s.sort(0, true);
        assert_eq!(text(&s), "| n |\n|---|\n| [b](http://x) |\n| A |\n| **10** |\n| 9 |\n|  |");
    }
}
