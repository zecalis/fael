//! Issue 01M5QF7Q: a done whose run has no branch (marked after its PR merged) claims the
//! merged PR's files, read from the squash commit on the trunk.

use std::path::Path;

/// The files of the commit on `trunk` whose subject ends `(#n)` (the squash-merge title);
/// None = no such commit or no files.
pub(crate) fn merged_files(dir: &Path, trunk: &str, n: i64) -> Option<Vec<String>> {
    let re = format!(r"\(#{n}\)$");
    let tag = format!("(#{n})");
    // --grep also matches body lines ("follow-up to (#n)"): keep only a commit whose
    // subject ends with the tag
    let log = ["log", trunk, "--format=%H %s", "-E", "--grep", &re];
    let found = crate::git(dir, &log)?;
    let (sha, _) = found
        .lines()
        .filter_map(|l| l.split_once(' '))
        .find(|(_, s)| s.ends_with(&tag))?;
    let out = crate::git(dir, &["show", "--name-only", "--format=", sha])?;
    let files: Vec<String> = out
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    (!files.is_empty()).then_some(files)
}

/// The line `done` prints when nothing was claimed: a fact, never a refusal.
pub(crate) fn none_claimed(trunk: &str, pr: Option<i64>) -> String {
    let named = pr.map(|n| format!(" names (#{n})")).unwrap_or_default();
    format!(
        "no diff claimed: no run branch and no commit on {trunk}{named} — the next brief prints no touched files; name the paths in the handoff\n"
    )
}
