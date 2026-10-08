//! A plan kickoff shows the open work, not the finished chunks: rows keyed
//! `plan:<name>:chunk-<n>` leave once the plan doc's Progress ticks chunk `n`.

use crate::Row;

/// Rows a plan kickoff shows when no `--limit` is given.
pub const PLAN_KICKOFF_ROWS: usize = 5;

/// The chunk numbers a plan doc's Progress ticked done (`- [x] chunk 2 …`) or
/// dropped (`- [~] chunk 3 …`). Read from the doc, never guessed: an unticked
/// or unnumbered line closes nothing.
pub fn closed_chunks(doc: &str) -> Vec<u32> {
    doc.lines()
        .filter_map(|l| {
            let l = l.trim_start().strip_prefix("- [")?;
            let (mark, l) = l.split_at_checked(1)?;
            if !matches!(mark, "x" | "X" | "~") {
                return None;
            }
            let n = l.strip_prefix("] chunk ")?;
            let end = n.find(|c: char| !c.is_ascii_digit()).unwrap_or(n.len());
            // `2a` is no chunk number — never guess
            if n[end..].starts_with(|c: char| c.is_alphanumeric()) {
                return None;
            }
            n[..end].parse().ok()
        })
        .collect()
}

/// `rows` minus those keyed to a closed chunk of plan `anchor` (`plan:<name>`).
/// The `:handoff` key and every row with another key stay.
pub fn drop_closed<'a>(rows: Vec<&'a Row>, anchor: &str, closed: &[u32]) -> Vec<&'a Row> {
    let done = |r: &Row| {
        r.key
            .as_deref()
            .and_then(|k| k.strip_prefix(anchor)?.strip_prefix(":chunk-"))
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|n| closed.contains(&n))
    };
    rows.into_iter().filter(|r| !done(r)).collect()
}

/// The plan's `:handoff` row first, so the cap never cuts the row a plan
/// kickoff exists to show; the rest keep their rank.
pub fn handoff_first<'a>(mut rows: Vec<&'a Row>, anchor: &str) -> Vec<&'a Row> {
    let handoff = format!("{anchor}:handoff");
    rows.sort_by_key(|r| r.key.as_deref() != Some(handoff.as_str())); // stable
    rows
}
