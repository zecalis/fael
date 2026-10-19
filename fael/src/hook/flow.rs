//! Flow inputs for `fael stats` (PLAN-fael-flow chunk f1): this clone's runs
//! from fael.db and its merged PRs from `gh` plus `git log`. Both readers
//! live here so `usage.rs` stays under the file-size cap; core joins them
//! (pure) in `fael-core::stats::flow`.

use crate::core;

/// One run for the stats flow join, from this clone's fael.db — the caller
/// converts the stored RFC 3339 times to ms, so core stays string-free.
pub(crate) fn runs() -> Vec<core::stats::FlowRun> {
    let db = crate::repo()
        .ok()
        .map(|r| core::work::db_for(r.journal.as_deref(), &r.fael))
        .filter(|p| p.exists());
    let Some(path) = db else { return vec![] };
    let runs = core::work::flow_runs(&path);
    runs.into_iter()
        .map(|r| core::stats::FlowRun {
            started_ms: core::ts_ms(&r.started),
            ended_ms: r.ended.as_deref().and_then(core::ts_ms),
            pr: r.pr,
            chunk: r.chunk,
        })
        .collect()
}

/// Merged PRs on this clone's default branch: `gh`'s merged list plus the
/// squash subjects in `git log` (`(#N)` + committer date) — matched by PR
/// number only, never a sha. `gh` failing leaves the `git log` fallback;
/// both failing leaves none (spans shrink, never impute).
pub(crate) fn merges(since_ms: Option<i64>) -> Vec<core::stats::FlowMerge> {
    let Ok(r) = crate::repo() else { return vec![] };
    let root = &r.root;
    let mut out: Vec<core::stats::FlowMerge> = vec![];
    if let Ok(gh) = std::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "merged",
            "--json",
            "mergedAt,number",
            "--limit",
            "500",
        ])
        .current_dir(root)
        .output()
        && gh.status.success()
        && let Ok(list) = serde_json::from_slice::<Vec<serde_json::Value>>(&gh.stdout)
    {
        for m in &list {
            if let (Some(n), Some(at)) = (
                m.get("number").and_then(|n| n.as_i64()),
                m.get("mergedAt")
                    .and_then(|s| s.as_str())
                    .and_then(core::ts_ms),
            ) {
                out.push(core::stats::FlowMerge {
                    number: n,
                    at_ms: at,
                });
            }
        }
    }
    let branch = crate::maintain::default_branch(root);
    let since = since_ms.map(|ms| format!("--since={}", core::rfc3339(ms.max(0) as u64)));
    let log = |r: &str| {
        let mut a: Vec<&str> = vec!["log", r, "--no-merges", "--format=%s%x1f%b%x1f%cI%x1e"];
        if let Some(s) = since.as_deref() {
            a.push(s);
        }
        crate::git(root, &a)
    };
    // the fetched base first, else the local one
    let text = log(&format!("origin/{branch}"))
        .or_else(|| log(&branch))
        .unwrap_or_default();
    for c in text.split('\x1e') {
        let mut parts = c.trim().split('\x1f');
        let (Some(subject), Some(body), Some(at)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Some(ms) = core::ts_ms(at.trim()) else {
            continue;
        };
        let text = format!("{subject}\n{body}");
        for cite in core::stats::pr_cites(&text) {
            if let Ok(n) = cite[2..cite.len() - 1].parse::<i64>() {
                out.push(core::stats::FlowMerge {
                    number: n,
                    at_ms: ms,
                });
            }
        }
    }
    out
}
