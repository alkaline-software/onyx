//! The note's vertical measures, in one place. The canvas draws by them
//! in all three modes and the page by the same scale of sizes, so a note
//! is set one way wherever it shows.
//!
//! A line's height goes by what kind of line it is and never by what
//! stands in it. Space belongs to the block it stands around, never to
//! its neighbor, so a word typed on a blank line moves nothing.

/// The body's line, as a share of the words' size.
pub const LINE: f32 = 1.6;
/// Code's line, as a share of code's size.
pub const CODE_LINE: f32 = 1.5;
/// A table's words, as a share of the body's size, and their line.
pub const CELL: f32 = 0.875;
pub const CELL_LINE: f32 = 1.5;
/// The caret, as a share of the size of the words it stands in: the
/// words' own height, without the room between lines. It goes by the size
/// alone, so it is one height on a blank line, in Latin and in Chinese.
pub const CARET: f32 = 1.2;
/// The note's name, as a share of the words' size, and its line: a step
/// over the top heading, so the name leads a note that opens with one.
pub const TITLE: f32 = 1.5;
pub const TITLE_LINE: f32 = 1.25;

/// A heading's measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heading {
    /// Its size, as a share of the words'.
    pub scale: f32,
    /// Its line, as a share of its size.
    pub line: f32,
    /// The room over it, as a share of the body's line.
    pub above: f32,
}

/// The six levels, in Craft's steps down to the words' size (its title,
/// subtitle, heading and strong text: 24, 22, 20 and 18 over 17), so a
/// heading leads its section without shouting over it; the last two are
/// the words' size, apart by weight.
pub const HEADINGS: [Heading; 6] = [
    Heading { scale: 1.412, line: 1.25, above: 0.5 },
    Heading { scale: 1.294, line: 1.25, above: 0.5 },
    Heading { scale: 1.176, line: 1.3, above: 0.4 },
    Heading { scale: 1.059, line: 1.35, above: 0.3 },
    Heading { scale: 1.0, line: 1.4, above: 0.2 },
    Heading { scale: 1.0, line: 1.5, above: 0.2 },
];

/// The fewest dashes a column of a table narrower than the room is
/// written with, so a table typed by hand (`| --- |`) is not read as one.
pub const LEAST_WIDTH: u32 = 5;

/// A table's widths, as a person set them: each column's share of the
/// room in hundredths, its dashes in the separator row. Adding up to 100,
/// the table spans the room; less, it stands that much of it, each column
/// its own share.
///
/// Under 100 a separator says widths only where no hand or formatter
/// would have written it so: every column has `LEAST_WIDTH` dashes or
/// more, not all the same count (a table typed gives each column the same
/// dashes), and the rows do not line up with it (`rows`, the table's
/// other lines, its head first: a formatter pads every row and the
/// separator to one length, the dashes being its columns' letters), nor
/// does it stand under the head cell for cell, which is how a hand or a
/// model rules a table off whatever its rows hold. Anything else says
/// nothing, and the columns go by their words.
pub fn widths_of(separator: &str, rows: &[&str]) -> Option<Vec<f32>> {
    let row = separator.trim_start_matches(|c: char| c == '>' || c.is_whitespace()).trim_end();
    let row = row.strip_prefix('|').unwrap_or(row);
    let row = row.strip_suffix('|').unwrap_or(row);
    let dashes: Vec<u32> = row.split('|').map(|c| c.chars().filter(|c| *c == '-').count() as u32).collect();
    if dashes.len() < 2 {
        return None;
    }
    let sum: u32 = dashes.iter().sum();
    let set = sum == 100 || (sum < 100 && dashes.iter().all(|d| *d >= LEAST_WIDTH) && dashes.iter().any(|d| *d != dashes[0]) && !lined_up(separator, rows) && !under_head(separator, rows));
    set.then(|| dashes.iter().map(|d| *d as f32).collect())
}

/// Whether every row is as long as the separator, as a monospace screen
/// shows them: a table a formatter padded.
fn lined_up(separator: &str, rows: &[&str]) -> bool {
    let at = long(separator.trim_end());
    !rows.is_empty() && rows.iter().all(|r| long(r.trim_end()) == at)
}

/// Whether the separator's cells are each as long as the head's over
/// them, bar under bar: a table ruled off under its head, as one typed or
/// written by a model is, its rows left as long as their words.
fn under_head(separator: &str, rows: &[&str]) -> bool {
    let Some(head) = rows.first() else { return false };
    let (rule, head) = (cell_lengths(separator), cell_lengths(head));
    rule.len() > 1 && rule == head
}

/// How long a line is on a monospace screen.
fn long(text: &str) -> usize {
    text.chars().map(columns).sum()
}

/// The columns a character takes there.
fn columns(c: char) -> usize {
    if wide(c) { 2 } else { 1 }
}

/// A row's cells' lengths between its bars, spaces and all, a bar after a
/// backslash being a cell's own.
fn cell_lengths(row: &str) -> Vec<usize> {
    let row = row.trim_start_matches(|c: char| c == '>' || c.is_whitespace()).trim_end();
    let row = row.strip_prefix('|').unwrap_or(row);
    let row = row.strip_suffix('|').unwrap_or(row);
    let mut out = vec![0];
    let mut chars = row.chars();
    while let Some(c) = chars.next() {
        match c {
            '|' => out.push(0),
            '\\' => *out.last_mut().unwrap() += 1 + chars.next().map_or(0, columns),
            _ => *out.last_mut().unwrap() += columns(c),
        }
    }
    out
}

/// A character a monospace screen gives two columns: CJK, full-width
/// forms and emoji, as a formatter counts them.
fn wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF | 0x20000..=0x3FFFD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_smaller_heading_never_asks_for_more_room() {
        for pair in HEADINGS.windows(2) {
            assert!(pair[0].scale >= pair[1].scale && pair[0].above >= pair[1].above);
        }
    }
}
