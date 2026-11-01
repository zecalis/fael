//! Default-branch commits for the stats join, read by `fael stats` and never
//! on a push. Split out of `usage.rs` (file-size cap).

use crate::core;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Each repo's default-branch commits since its first usage (H5 capture
/// recall), read by `fael stats`, never on a push. Worktrees of one clone
/// share its object store, so the clone gets one `git log` from its earliest
/// first usage and each worktree keeps the commits since its own.
pub(super) fn commits_by_clone(u: &super::usage::Usage) -> core::stats::Commits {
    let mut clones: HashMap<PathBuf, Vec<(&String, i64)>> = HashMap::new();
    for repo in u.logs.keys() {
        if let Some(&since) = u.parsed.first_seen.get(repo) {
            let key = crate::journal::root(Path::new(repo)).unwrap_or_else(|| repo.into());
            clones.entry(key).or_default().push((repo, since));
        }
    }
    let mut out = core::stats::Commits::new();
    for repos in clones.values() {
        let since = repos.iter().map(|(_, s)| *s).min().unwrap_or(0);
        let all = commits(Path::new(repos[0].0), since);
        for (repo, since) in repos {
            let mine = all.iter().filter(|(at, _)| *at >= *since);
            out.insert((*repo).clone(), mine.map(|(_, c)| c.clone()).collect());
        }
    }
    out
}

/// The default branch's commits since `since_ms`, each with its committer
/// time (what `--since` filters on). No git, no branch: none.
pub(super) fn commits(root: &Path, since_ms: i64) -> Vec<(i64, core::stats::Commit)> {
    let branch = crate::maintain::default_branch(root);
    let since = format!("--since={}", core::rfc3339(since_ms.max(0) as u64));
    let log = |r: &str| {
        let fmt = "--format=%H%x1f%cI%x1f%B%x1e";
        crate::git(root, &["log", r, "--no-merges", &since, fmt])
    };
    let out = log(&format!("origin/{branch}")).or_else(|| log(&branch));
    out.unwrap_or_default()
        .split('\x1e')
        .filter_map(|c| {
            let mut p = c.trim().splitn(3, '\x1f');
            let (sha, at, message) = (p.next()?, p.next()?, p.next()?);
            let commit = core::stats::Commit {
                sha: sha.to_string(),
                message: message.trim().to_string(),
            };
            Some((core::ts_ms(at)?, commit))
        })
        .collect()
}
