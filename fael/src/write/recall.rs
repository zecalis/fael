//! Closed-issue recall (PLAN-fael-context-loop chunk 5): a new issue on a file
//! that already carries a closed one says so, with the command to link them.
//! Only the agent can say it is the same bug — fael names the candidates (at
//! most two, newest first) and never links them itself. Silent without one.

use crate::{core, hook};

const MAX: usize = 2;

/// Info lines for the issue just filed (`log` is the log before it). None when
/// the row is no issue, already supersedes something (the caller linked it, or
/// self-heal did), or no closed issue shares a file with it.
pub(crate) fn lines(log: &core::Log, row: &core::Row) -> Vec<String> {
    if row.kind != "issue" || row.supersedes.is_some() {
        return vec![];
    }
    let gone = core::closed(log);
    let linked = core::superseded(log);
    let mut hits: Vec<&core::Row> = log
        .rows
        .iter()
        .filter(|r| r.kind == "issue" && gone.contains(r.id.as_str()))
        .filter(|r| !linked.contains(r.id.as_str()))
        .filter(|r| {
            r.files
                .iter()
                .any(|f| row.files.contains(f) && !hook::is_anchor(f) && !core::is_glob(f))
        })
        .collect();
    hits.sort_by(|a, b| b.id.cmp(&a.id)); // ids are time-ordered
    let w = core::abbrev(log).with(&row.id);
    let new = w.short(&row.id);
    hits.iter()
        .take(MAX)
        .map(|c| {
            let old = w.short(&c.id);
            format!(
                "closed issue on these files: {old} \"{}\" — the same bug back? \
fael add issue \"<text>\" --supersedes {old}, then fael close {new} \"refiled\"",
                c.display_title()
            )
        })
        .collect()
}
