//! `find --kind issue` tags: `[Gone]` on rows whose files are all gone,
//! `(merged)` and `(files changed since)` on rows whose branch already landed
//! or whose files moved. The merge mark is kickoff's `merged::mark` channel —
//! one tag channel, no second writer: an issue fixed on another branch has no
//! other sign (issue find:issue-changed).

use crate::journal::BranchMap;
use crate::{aliases, core};
use std::collections::HashSet;

/// For an issue list, the branch tags with `(merged)` and `(files changed since)` added through
/// kickoff's git-proven channel, and the ids `[Gone]` applies to — the same
/// judgement `doctor` uses (resolver + the row's own branch still holding the
/// file = not gone).
pub(crate) fn marks(
    r: &crate::Repo,
    log: &core::Log,
    is_issue: bool,
    rows: &[&core::Row],
    branch_of: BranchMap,
) -> (BranchMap, HashSet<String>) {
    if !is_issue {
        return (branch_of, HashSet::new());
    }
    let branch_of = super::merged::mark(&r.root, branch_of, rows);
    let al = aliases::load(r, log, true);
    let branch_of =
        super::merged::mark_changed(&r.root, branch_of, rows, &al, &r.cfg.anchor_prefixes);
    let gone = crate::maintain::gone_ids(&r.root, &al, rows);
    (branch_of, gone)
}

/// Suffix ` [Gone]` on every rendered row line whose full id is gone.
/// Header and hint lines never start with `- [`, so they pass through.
pub(crate) fn tag_gone(out: &str, gone: &HashSet<String>) -> String {
    if gone.is_empty() {
        return out.to_string();
    }
    let mut tagged = String::with_capacity(out.len());
    for line in out.split_inclusive('\n') {
        let short = line
            .strip_prefix("- [")
            .and_then(|l| l.split(']').next())
            .filter(|s| s.len() >= 8);
        let is_gone = short.is_some_and(|s| gone.iter().any(|id| id.starts_with(s)));
        if is_gone {
            let (body, nl) = line
                .strip_suffix('\n')
                .map(|l| (l, "\n"))
                .unwrap_or((line, ""));
            tagged.push_str(&format!("{body} [Gone]{nl}"));
        } else {
            tagged.push_str(line);
        }
    }
    tagged
}
