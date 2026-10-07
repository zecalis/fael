//! `find --kind issue` default surface (PLAN-fael-agent-ergonomics chunk 4):
//! the grouped list, with `[Gone]` on rows whose files are all gone and
//! `(merged)` on rows whose branch already landed. The grouping is the
//! `--groups` renderer, the merge mark is kickoff's `merged::mark` channel —
//! one tag channel, no second writer (PLAN-fael-file-hash chunk 5a shares it
//! for `(files changed since)` on kickoff handoffs, never on issues).

use super::branches::BranchMap;
use crate::{Args, aliases, core};
use std::collections::HashSet;

/// The plain issue list wants the grouped answer without asking for it:
/// unpaged text, no machine shape. An explicit `--full`/`--limit`/`--offset`
/// keeps the flat list — the caller asked for a slice, not the whole picture.
pub(crate) fn auto_grouped(a: &Args, f: &core::Filter) -> bool {
    f.kind.as_deref() == Some("issue")
        && !a.has("json")
        && !a.has("full")
        && f.limit.is_none()
        && f.offset == 0
}

/// `find --groups`, and the auto-grouped issue list: every match, grouped by
/// shared files — what to fix in one PR. Unpaged and unbudgeted: half a group
/// answers the question wrong.
pub(crate) fn groups(
    a: &Args,
    log: &core::Log,
    f: &core::Filter,
    branch_of: BranchMap,
    r: &crate::Repo,
) -> Result<(), String> {
    if a.has("json") || a.has("full") || f.limit.is_some() || f.offset > 0 {
        return Err(
            "rejected: --groups lists every match as text — drop --json, --full, --limit and --offset"
                .into(),
        );
    }
    let rows = core::find(
        log,
        &core::Filter {
            limit: None,
            ..f.clone()
        },
    );
    if rows.is_empty() {
        eprintln!("fael: no rows match");
    } else if f.kind.as_deref() == Some("issue") {
        print!("{}", render_issue_groups(r, log, &rows, branch_of));
    } else {
        print!(
            "{}",
            super::branches::tag(core::render_groups(log, &rows), &branch_of)
        );
    }
    Ok(())
}

/// The grouped issue list with its two tags: `(merged)` through kickoff's
/// git-proven channel, `[Gone]` through the same judgement `doctor` uses
/// (resolver + the row's own branch still holding the file = not gone).
pub(crate) fn render_issue_groups(
    r: &crate::Repo,
    log: &core::Log,
    rows: &[&core::Row],
    branch_of: BranchMap,
) -> String {
    let branch_of = super::merged::mark(&r.root, branch_of, rows);
    let al = aliases::load(r, log, true);
    let gone = crate::maintain::gone_ids(&r.root, &al, rows);
    let out = super::branches::tag(core::render_groups(log, rows), &branch_of);
    tag_gone(&out, &gone)
}

/// Suffix ` [Gone]` on every rendered row line whose full id is gone.
/// Header and hint lines never start with `- [`, so they pass through.
fn tag_gone(out: &str, gone: &HashSet<String>) -> String {
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
