//! Flow inputs for `fael stats` (PLAN-fael-flow chunk f1): this clone's runs
//! from crew.db and its merged PRs from `gh` plus `git log`. Both readers
//! live here so `usage.rs` stays under the file-size cap; core joins them
//! (pure) in `fael-core::stats::flow`.

use crate::core;

/// One run for the stats flow join, from this clone's crew.db — the caller
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
/// both failing leaves none (spans shrink, never impute). The `gh` half is
/// cached per process (issue 01M5F53Y): repeated `stats` in one run share
/// the one network call.
pub(crate) fn merges(since_ms: Option<i64>) -> Vec<core::stats::FlowMerge> {
    let Ok(r) = crate::repo() else { return vec![] };
    let root = &r.root;
    let mut out: Vec<core::stats::FlowMerge> = vec![];
    for (n, at) in gh_merged(root) {
        out.push(core::stats::FlowMerge {
            number: n,
            at_ms: at,
        });
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

/// The one `gh pr list --state merged` call per process, shared by repeated
/// `stats` in one run. `(number, merged_at_ms)`, empty when `gh` is missing
/// or fails — the `git log` fallback in `merges` still applies.
fn gh_merged(root: &std::path::Path) -> Vec<(i64, i64)> {
    static CACHE: std::sync::OnceLock<Vec<(i64, i64)>> = std::sync::OnceLock::new();
    // the cache is per process, not per repo: `stats` reads one clone per
    // run, so the first root wins — same single-clone assumption as `runs`.
    CACHE
        .get_or_init(|| {
            let Some(stdout) = gh_raw(
                root,
                "gh-merged.json",
                &[
                    "pr",
                    "list",
                    "--state",
                    "merged",
                    "--json",
                    "mergedAt,number",
                    "--limit",
                    "500",
                ],
            ) else {
                return vec![];
            };
            let Ok(list) = serde_json::from_slice::<Vec<serde_json::Value>>(&stdout) else {
                return vec![];
            };
            let mut merged = vec![];
            for m in &list {
                if let (Some(n), Some(at)) = (
                    m.get("number").and_then(|n| n.as_i64()),
                    m.get("mergedAt")
                        .and_then(|s| s.as_str())
                        .and_then(core::ts_ms),
                ) {
                    merged.push((n, at));
                }
            }
            merged
        })
        .clone()
}

/// `gh`'s raw list for `args`, kept on disk next to `usage.jsonl` for `GH_TTL`:
/// the network call is most of `stats --json` (~3 s of ~7 s) and a board
/// refreshes it every few seconds. First line = the clone it was read for.
pub(crate) fn gh_raw(root: &std::path::Path, name: &str, args: &[&str]) -> Option<Vec<u8>> {
    const GH_TTL: std::time::Duration = std::time::Duration::from_secs(600);
    let file = super::usage_files::live().with_file_name(name);
    let head = format!("{}\n", root.display());
    let fresh = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < GH_TTL);
    if fresh
        && let Ok(cached) = std::fs::read(&file)
        && let Some(body) = cached.strip_prefix(head.as_bytes())
    {
        return Some(body.to_vec());
    }
    let out = std::process::Command::new("gh")
        .args(args)
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    // a failed write only costs the next call a refetch
    let _ = std::fs::write(&file, [head.as_bytes(), &out.stdout].concat());
    Some(out.stdout)
}
