//! `--revisit` (PLAN-fael-row-hygiene chunk 5): a row carries a date
//! (`YYYY-MM` or `YYYY-MM-DD`) or free text (`mdl lands`). A date ≤ today is
//! *due*: `kickoff` lists it first, even from outside the file filter, so a
//! sleeping row wakes up on time. Free text never lists — kickoff counts it
//! (`fael find --revisit` shows it).
//!
//! Dates compare as strings: zero-padded `YYYY-MM[-DD]` sorts
//! chronologically, and a bare `YYYY-MM` is a prefix of any day inside it,
//! so plain `<=` against `today` (`YYYY-MM-DD`) does the job.

use super::Filter;
use crate::{Aliases, Log, Row};
use std::collections::HashSet;
use std::path::Path;

/// A `YYYY-MM` or `YYYY-MM-DD` date — zero-padded, month 01–12, day 01–31.
/// Anything else (`mdl lands`, `2026-9`) is free text.
pub fn is_date(s: &str) -> bool {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() != 7 && b.len() != 10 {
        return false;
    }
    if b[4] != b'-' || (b.len() == 10 && b[7] != b'-') {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(|c| c.is_ascii_digit());
    if !digits(0..4) || !digits(5..7) {
        return false;
    }
    // byte slices are ASCII digits, so these are char boundaries
    if !(1..=12).contains(&s[5..7].parse().unwrap_or(0)) {
        return false;
    }
    if b.len() == 10 {
        if !digits(8..10) {
            return false;
        }
        if !(1..=31).contains(&s[8..10].parse().unwrap_or(0)) {
            return false;
        }
    }
    true
}

/// Today as `YYYY-MM-DD`, from the row clock (`rfc3339`).
pub fn today() -> String {
    crate::rfc3339(crate::now_ms())
        .get(..10)
        .unwrap_or("")
        .into()
}

/// A due revisit: a date, and not later than `today`.
pub fn due(revisit: &str, today: &str) -> bool {
    let r = revisit.trim();
    is_date(r) && r <= today
}

/// The row's revisit is due as of `today` — blank and free text never are.
pub fn row_due(r: &Row, today: &str) -> bool {
    r.revisit().is_some_and(|v| due(v, today))
}

/// Open rows whose revisit is free text — kickoff counts them,
/// `find --revisit` lists them. Rows whose files are all gone are dropped,
/// the way kickoff drops them: the count must point at rows the list shows.
pub fn waiting<'a>(log: &'a Log, root: &Path, al: &Aliases) -> Vec<&'a Row> {
    super::find(log, &Filter::default())
        .into_iter()
        .filter(|r| {
            r.revisit()
                .is_some_and(|v| !v.trim().is_empty() && !is_date(v))
                && !super::gone(root, r, al)
        })
        .collect()
}

/// The count line kickoff prints for free-text revisits (due dates list in
/// full, so they need no line).
pub fn waiting_line(n: usize) -> String {
    format!(
        "fael: {n} {} waiting on revisit — fael find --revisit (MCP find revisit=true)\n",
        if n == 1 { "row" } else { "rows" }
    )
}

/// Split a kickoff set into due rows first, then the rest. Due rows filed
/// under other paths still wake up: open only (`find` already hides closed,
/// superseded and alias rows), and never gone ones. Both halves come back
/// for the caller to rank.
pub fn with_due<'a>(
    log: &'a Log,
    base: Vec<&'a Row>,
    root: &Path,
    al: &Aliases,
) -> (Vec<&'a Row>, Vec<&'a Row>) {
    let day = today();
    let mut seen: HashSet<&str> = base.iter().map(|r| r.id.as_str()).collect();
    let mut due: Vec<&Row> = base.iter().filter(|r| row_due(r, &day)).copied().collect();
    for r in super::find(log, &Filter::default()) {
        if seen.insert(r.id.as_str()) && row_due(r, &day) && !super::gone(root, r, al) {
            due.push(r);
        }
    }
    let rest: Vec<&Row> = base.into_iter().filter(|r| !row_due(r, &day)).collect();
    (due, rest)
}
