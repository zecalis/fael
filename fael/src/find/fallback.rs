//! An empty open-only find answers with the closed rows the same call with
//! `--all` would match — one header line, each row marked by render
//! (`(closed)`, `(dormant)`, `(retired)`). Exact `--all` only: no fuzzy
//! matching, no partial-word guesses, no slug splitting, never a guess.
//!
//! The flat find already answers through `core::nearest` (closed first);
//! this module is the same answer for the grouped surfaces, which today say
//! only `why_empty`.

use crate::core::{self, Filter, Log, Row};

/// The closed rows the same call with `--all` would match, under the header
/// saying no open row matched. `None` when the caller passed `--all`, past
/// the first page, or nothing matches even closed.
pub(crate) fn closed_rows<'a>(log: &'a Log, f: &Filter) -> Option<(String, Vec<&'a Row>)> {
    core::closed_fallback(log, f)
}

/// The grouped answer over fallback rows: the issue groups for an issue
/// list, the file groups otherwise — the same surface a non-empty answer
/// prints. Takes `branch_of` by value like the non-empty path.
pub(crate) fn grouped_text(
    r: &crate::Repo,
    log: &Log,
    rows: &[&Row],
    branch_of: crate::journal::BranchMap,
    is_issue: bool,
) -> String {
    if is_issue {
        super::issue::render_issue_groups(r, log, rows, branch_of)
    } else {
        crate::journal::tag(core::render_groups(log, rows), &branch_of)
    }
}

/// Fallback rows were shown: record them like the flat find does, so usage
/// counts the answer and the next push skips what this call already said.
pub(crate) fn record_shown(r: &crate::Repo, f: &Filter, shown: &[String]) {
    crate::hook::record_found(
        "cli",
        "find",
        &r.root,
        shown,
        (f.key.as_deref(), &f.files, None),
    );
    if !f.files.is_empty() {
        let ids: Vec<&str> = shown.iter().map(String::as_str).collect();
        crate::hook::note_seen(&crate::session::hook_session(&r.root), &r.root, &ids);
    }
}
