//! Provenance for `fael stats` (plan proof chunk `provenance`): every
//! git/gh-backed number carries the shell line that recomputes it.
//!
//! Pure: the caller fills in this run's range, git-since day and default
//! branch, so core spawns nothing and reads no clock. `fael stats
//! --json | jq .provenance` prints the map; `fael stats provenance`
//! prints the same lines as text. Numbers computed from `usage.jsonl`
//! alone are not shell lines: their value is `from usage lines, see
//! docs/stats.md#<field>`, where one `jq` filter says it in one line.

use std::collections::BTreeMap;

/// The `gh pr list` call behind the listed-PR joins: the source of
/// `prs_created`, `prs_with_row`/`without` and `joined_since`. Capped at
/// 1000 (the stats window pages to the window start; doctor keeps 200).
const PR_LIST: &str = "gh pr list --state all --json number,headRefName,createdAt,mergedAt --limit 1000";

/// Field path → the exact shell line with this run's range filled in.
///
/// `range` is `window-day..today` in the viewer's zone (the string
/// `prs_merged_count` counts, so a check recomputes the manual count over
/// the same string), `None` when unwindowed. `git_since` is the RFC 3339
/// instant the `git log` starts from, `default_branch` the remote's
/// default (`origin/<branch>`, else the local one), `window_day` the
/// `YYYY-MM-DD` the outcome window starts on (`"all"`/`""` when none).
pub fn provenance(
    range: Option<&str>,
    git_since: &str,
    default_branch: &str,
    window_day: &str,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert("outcome.prs_merged".into(), prs_merged(range));
    out.insert(
        "outcome.prs_created".into(),
        format!(
            "{PR_LIST} --jq '[.[] | select(.createdAt != null{})] | length'{}",
            created_filter(window_day),
            window_note(window_day),
        ),
    );
    out.insert(
        "outcome.joined_since".into(),
        format!(
            "{PR_LIST} --jq -r '[.[].createdAt, .[].mergedAt] | map(select(. != null)) | min | .[0:10]'"
        ),
    );
    out.insert(
        "outcome.prs_with_row".into(),
        format!(
            "{PR_LIST} # listed set; the with-row subset is joined in fael-core/src/stats/outcome.rs (close (#N), commit (fael:prefix)+(#N), branch=headRefName)"
        ),
    );
    out.insert(
        "outcome.prs_without_row".into(),
        format!(
            "{PR_LIST} # listed set; without = listed - outcome.prs_with_row (join in fael-core/src/stats/outcome.rs)"
        ),
    );
    out.insert(
        "experience.fix_commits".into(),
        format!(
            "git log origin/{default_branch} --no-merges --since={git_since} --format=%s | grep -c -E '^fix(\\(|:'"
        ),
    );
    out.insert(
        "experience.fix_commits_linked".into(),
        format!(
            "git log origin/{default_branch} --no-merges --since={git_since} --format=%H\\ %s | grep -E '^\\S+ fix(\\(|:' | grep -E '\\(fael:|\\(#[0-9]+\\)' | wc -l # linked subset: message names a row id, or a close names its sha/(#N); full rule in fael-core/src/stats/experience.rs"
        ),
    );
    out.insert(
        "context_loop.confirmed_repeats".into(),
        format!(
            "jq -r 'select(.kind==\"issue\" and .supersedes != null) | .id' $(git rev-parse --git-common-dir)/fael/*.jsonl .fael/log/*.jsonl 2>/dev/null | sort -u | wc -l # repeats whose superseded issue is already closed; window {window_day}; full rule in fael-core/src/stats/repeat.rs"
        ),
    );
    out.insert(
        "context_loop.repeats".into(),
        out["context_loop.confirmed_repeats"].clone(),
    );
    out
}

/// The `gh api search/issues` count over the range — the whole range, not
/// the listed rows. `None` (unwindowed) keeps the template with `<range>`
/// so the reader sees what `--since` would fill in.
fn prs_merged(range: Option<&str>) -> String {
    match range {
        Some(r) => format!(
            "gh api \"search/issues?q=repo:{{{{owner}}}}/{{{{repo}}}}+type:pr+is:merged+merged:{r}&per_page=1\" --jq .total_count"
        ),
        None => "gh api \"search/issues?q=repo:{{owner}}/{{repo}}+type:pr+is:merged+merged:<YYYY-MM-DD..YYYY-MM-DD>&per_page=1\" --jq .total_count # unwindowed: pass --since YYYY-MM-DD to set <range>".into(),
    }
}

/// The `createdAt` filter for the window day: a date filters, `"all"`/`""`
/// counts every readable time (the same rule `outcome` uses).
fn created_filter(window_day: &str) -> String {
    if window_day.len() == 10 && window_day.as_bytes()[4] == b'-' {
        format!(" and .createdAt >= \"{window_day}\"")
    } else {
        String::new()
    }
}

/// Human note beside the `prs_created` line naming the window it counted.
fn window_note(window_day: &str) -> String {
    match window_day {
        "all" => " # window all (--since all)".into(),
        "" => " # unwindowed".into(),
        d => format!(" # window {d}"),
    }
}

#[cfg(test)]
mod tests {
    use super::provenance;

    #[test]
    fn every_named_field_maps_to_a_shell_line() {
        let p = provenance(
            Some("2026-10-01..2026-11-12"),
            "2026-10-01T00:00:00Z",
            "main",
            "2026-10-01",
        );
        for k in [
            "outcome.prs_merged",
            "outcome.prs_with_row",
            "experience.fix_commits_linked",
            "context_loop.repeats",
        ] {
            assert!(p.contains_key(k), "{k} missing: {:?}", p.keys());
            assert!(!p[k].trim().is_empty(), "{k}");
        }
        assert!(p["outcome.prs_merged"].contains("2026-10-01..2026-11-12"));
        assert!(p["experience.fix_commits"].contains("origin/main"));
        assert!(p["experience.fix_commits"].contains("--since=2026-10-01"));
    }

    #[test]
    fn unwindowed_keeps_a_template_not_an_empty_string() {
        let p = provenance(None, "1970-01-01T00:00:00Z", "main", "");
        assert!(p["outcome.prs_merged"].contains("<YYYY-MM-DD..YYYY-MM-DD>"));
    }
}
