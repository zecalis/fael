//! A plan kickoff: a short page with the plan's `:handoff` row first.

use crate::Row;

/// Rows a plan kickoff shows when no `--limit` is given.
pub const PLAN_KICKOFF_ROWS: usize = 5;

/// The plan's `:handoff` row first, so the cap never cuts the row a plan
/// kickoff exists to show; the rest keep their rank.
pub fn handoff_first<'a>(mut rows: Vec<&'a Row>, anchor: &str) -> Vec<&'a Row> {
    let handoff = format!("{anchor}:handoff");
    rows.sort_by_key(|r| r.key.as_deref() != Some(handoff.as_str())); // stable
    rows
}
