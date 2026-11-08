//! A note changed in two places at once, joined: what the window has
//! typed since the note was last written, and what another app wrote to
//! the file meanwhile. Each side's changes are found line by line against
//! the text both began from, and every one is kept. Where both changed
//! the same lines differently, the window's lines come first and the
//! disk's follow, so no word of either is lost.

use similar::{capture_diff_slices, Algorithm, DiffOp};

/// A stretch of the base, `from..to` in lines, replaced by `lines`.
struct Hunk<'a> {
    from: usize,
    to: usize,
    lines: Vec<&'a str>,
}

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn hunks<'a>(base: &[&'a str], other: &[&'a str]) -> Vec<Hunk<'a>> {
    capture_diff_slices(Algorithm::Myers, base, other)
        .into_iter()
        .filter_map(|op| match op {
            DiffOp::Equal { .. } => None,
            DiffOp::Delete { old_index, old_len, .. } => Some(Hunk { from: old_index, to: old_index + old_len, lines: Vec::new() }),
            DiffOp::Insert { old_index, new_index, new_len } => Some(Hunk { from: old_index, to: old_index, lines: other[new_index..new_index + new_len].to_vec() }),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => Some(Hunk { from: old_index, to: old_index + old_len, lines: other[new_index..new_index + new_len].to_vec() }),
        })
        .collect()
}

/// The base's lines `from..to` with one side's hunks in that stretch put in.
fn side<'a>(base: &[&'a str], from: usize, to: usize, hunks: &[&Hunk<'a>]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut at = from;
    for h in hunks {
        out.extend_from_slice(&base[at..h.from]);
        out.extend_from_slice(&h.lines);
        at = h.to;
    }
    out.extend_from_slice(&base[at..to]);
    out
}

/// `mine` and `theirs`, both grown from `base`, as one text, and whether
/// any lines were changed on both sides differently.
pub fn merge(base: &str, mine: &str, theirs: &str) -> (String, bool) {
    if mine == base || mine == theirs {
        return (theirs.to_string(), false);
    }
    if theirs == base {
        return (mine.to_string(), false);
    }
    // Each text ends with a line break while it is compared, so a line
    // added after a last line without one is not a change to that line.
    let close = |t: &str| if t.ends_with('\n') || t.is_empty() { t.to_string() } else { format!("{t}\n") };
    let (b, m, t) = (close(base), close(mine), close(theirs));
    let (bl, ml, tl) = (lines(&b), lines(&m), lines(&t));
    let (mh, th) = (hunks(&bl, &ml), hunks(&bl, &tl));
    let (mut i, mut j, mut at) = (0, 0, 0);
    let mut out: Vec<&str> = Vec::new();
    let mut both = false;
    while i < mh.len() || j < th.len() {
        let start = match (mh.get(i), th.get(j)) {
            (Some(a), Some(b)) => a.from.min(b.from),
            (Some(a), None) => a.from,
            (None, Some(b)) => b.from,
            (None, None) => break,
        };
        out.extend_from_slice(&bl[at..start]);
        // The hunks that touch one another, from either side, as a group.
        let (mut end, mut gm, mut gt) = (start, Vec::new(), Vec::new());
        loop {
            let takes = |h: &Hunk| h.from < end || h.from == start;
            if let Some(h) = mh.get(i).filter(|h| takes(h)) {
                end = end.max(h.to);
                gm.push(h);
                i += 1;
            } else if let Some(h) = th.get(j).filter(|h| takes(h)) {
                end = end.max(h.to);
                gt.push(h);
                j += 1;
            } else {
                break;
            }
        }
        match (gm.is_empty(), gt.is_empty()) {
            (false, true) => out.extend(side(&bl, start, end, &gm)),
            (true, false) => out.extend(side(&bl, start, end, &gt)),
            _ => {
                // What the two make alike at the start and the end is
                // written once; only the middle each made its own is twice.
                let (a, b) = (side(&bl, start, end, &gm), side(&bl, start, end, &gt));
                let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
                let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
                let (am, bm) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
                both |= !am.is_empty() && !bm.is_empty();
                out.extend_from_slice(&a[..head]);
                out.extend_from_slice(am);
                out.extend_from_slice(bm);
                out.extend_from_slice(&a[a.len() - tail..]);
            }
        }
        at = end;
    }
    out.extend_from_slice(&bl[at..]);
    let mut text = out.concat();
    if !mine.ends_with('\n') && !theirs.ends_with('\n') && text.ends_with('\n') {
        text.pop();
    }
    (text, both)
}

#[cfg(test)]
mod tests {
    use super::merge;

    #[test]
    fn changes_on_different_lines_are_both_kept() {
        let base = "one\ntwo\nthree\nfour\n";
        let mine = "one\ntwo, typed\nthree\nfour\n";
        let theirs = "one\ntwo\nthree\nfour, synced\nfive\n";
        assert_eq!(merge(base, mine, theirs), ("one\ntwo, typed\nthree\nfour, synced\nfive\n".to_string(), false));
    }

    #[test]
    fn the_same_line_changed_twice_keeps_both_mine_first() {
        let base = "a\nhello world\nz\n";
        let (text, both) = merge(base, "a\nhello brave world\nz\n", "a\nhello world!\nz\n");
        assert!(both);
        assert_eq!(text, "a\nhello brave world\nhello world!\nz\n");
    }

    #[test]
    fn the_same_change_on_both_sides_is_kept_once() {
        let base = "a\nb\n";
        assert_eq!(merge(base, "a\nB\nc\n", "a\nB\n"), ("a\nB\nc\n".to_string(), false));
        assert_eq!(merge(base, "a\nB\n", "a\nB\n").0, "a\nB\n");
    }

    #[test]
    fn a_last_line_without_a_break_takes_lines_after_it() {
        let (text, _) = merge("title", "title\nmine", "title\n\ntheirs");
        assert_eq!(text, "title\nmine\n\ntheirs");
        assert!(!text.ends_with('\n'));
    }

    #[test]
    fn one_side_unchanged_gives_the_other() {
        assert_eq!(merge("x\n", "x\n", "y\n").0, "y\n");
        assert_eq!(merge("x\n", "y\n", "x\n").0, "y\n");
    }
}
