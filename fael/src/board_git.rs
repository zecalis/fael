//! PLAN-fael-parallel: an open PR still holds its scope (less `.fael/shared`), and does its
//! branch still merge.
//! An open PR = a review chunk, or a done chunk with `--pr N` whose `(#N)` (squash subject)
//! or `Merge pull request #N` is not on the checkout's fetched base yet. Live git, no fetch
//! (the app fetches, decision 01M4K4GFS), no gh, never a state change.

use crate::git;
use fael_crew::{BoardChunk, overlaps, shared};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The base (`origin/HEAD`) and its subjects since the first shipped chunk started.
type Base = Option<(String, String)>;

/// Marks the open PRs and redoes `overlaps` with them and `.fael/shared`; returns that list.
/// `base` is `origin/HEAD`'s branch, read once per project by the caller.
pub(super) fn open_prs(chunks: &mut [BoardChunk], root: &Path, base: Option<&str>) -> Vec<String> {
    let shipped = |c: &BoardChunk| {
        c.run
            .as_ref()
            .and_then(|r| r.pr)
            .filter(|_| c.state == "done")
    };
    // ponytail: a PR closed unmerged keeps its scope until its plan is archived; gh would know
    let since = chunks
        .iter()
        .filter(|c| shipped(c).is_some())
        .filter_map(|c| c.run.as_ref().map(|r| r.started.clone()))
        .min();
    let base_name = base;
    let mut base: Option<Base> = None;
    let mut open = HashSet::new();
    for c in chunks.iter_mut() {
        let pr = shipped(c);
        if c.state != "review" && pr.is_none() {
            continue;
        }
        let base = base.get_or_insert_with(|| {
            let b = base_name?.to_string();
            let log = since
                .as_ref()
                .and_then(|s| git(root, &["log", "--format=%s", &format!("--since={s}"), &b]));
            Some((b, log.unwrap_or_default()))
        });
        let Some((base, log)) = base else {
            continue;
        };
        let branch = c.run.as_ref().and_then(|r| r.branch.clone());
        // a PR comes from a branch other than the base: a run with none, or on main, named
        // another repo's PR (the board app's, run by hand from this checkout)
        let pr_branch = branch
            .as_deref()
            .is_some_and(|b| Some(b) != base.strip_prefix("origin/"));
        if pr.is_some_and(|n| !pr_branch || merged(log, n)) {
            continue;
        }
        open.insert(c.uid.clone());
        if let Some(b) = branch {
            c.conflicts = merge_tree(root, base, &b);
        }
    }
    // a bad line never hides the board: said here, and by `fael doctor`
    let shared = shared(root).unwrap_or_else(|e| {
        eprintln!("fael board: {e}");
        Vec::new()
    });
    overlaps(chunks, &open, &shared);
    shared
}

/// A running run's branch is its worktree's now, not the one it started on (the agent may
/// switch after start), so the app's Talk (worktree branch == run branch) holds.
// ponytail: pooled worktrees only (a main checkout beside linked ones is not listed); done
// records the branch for those too
pub(super) fn live_branches(projects: &mut [super::Project]) {
    let now: HashMap<String, Option<String>> = projects
        .iter()
        .flat_map(|p| &p.worktrees)
        .map(|w| (w.path.clone(), w.branch.clone()))
        .collect();
    let live = projects
        .iter_mut()
        .flat_map(|p| &mut p.chunks)
        .filter_map(|c| c.run.as_mut())
        .filter(|r| r.ended.is_none());
    for r in live {
        if let Some(b) = r.worktree.as_ref().and_then(|w| now.get(w)) {
            r.branch.clone_from(b);
        }
    }
}

/// PR `n` on the base: a squash subject ending `(#n)`, or GitHub's merge commit.
fn merged(log: &str, n: i64) -> bool {
    let (squash, merge) = (format!("(#{n})"), format!("Merge pull request #{n} "));
    log.lines()
        .any(|s| s.contains(&squash) || s.starts_with(&merge))
}

/// `git merge-tree --write-tree` (git >= 2.38): 0 = clean, 1 with a tree id = the conflicted
/// paths after it; anything else (old git, no such branch) = unknown.
pub(crate) fn merge_tree(dir: &Path, base: &str, branch: &str) -> Option<Vec<String>> {
    let args = ["merge-tree", "--write-tree", "--name-only", "--no-messages"];
    let (code, out) = super::git_output(dir, &[&args[..], &[base, branch]].concat())?;
    let mut lines = out.lines();
    lines.next().filter(|tree| !tree.is_empty())?;
    match code? {
        0 => Some(Vec::new()),
        1 => Some(
            lines
                .take_while(|l| !l.is_empty())
                .map(String::from)
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::merged;

    #[test]
    fn a_pr_is_merged_by_its_number_only() {
        let log = "feat: x (#12)\nMerge pull request #7 from o/b\nfix: y (#123)";
        assert!(merged(log, 12) && merged(log, 7) && merged(log, 123));
        assert!(!merged(log, 1) && !merged(log, 23));
    }
}
