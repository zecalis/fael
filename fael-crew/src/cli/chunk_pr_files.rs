//! Issue 01M5QF7Q: a done whose run has no branch (marked after its PR merged) claims the
//! merged PR's files, read from the squash commit on the trunk. Issue 01M63XC1: a PR still
//! open has no such commit — its files are read through `gh`.

use std::path::Path;
use std::process::Command;

/// The files PR `n` changed: its squash commit on `trunk`, else (still open) `gh`'s list.
pub(crate) fn pr_files(dir: &Path, trunk: &str, n: i64, gh: &str) -> Option<Vec<String>> {
    merged_files(dir, trunk, n).or_else(|| open_files(dir, n, gh))
}

/// The files of PR `n` through `gh` (run in `dir`); None = gh missing or failing, or no files.
pub(crate) fn open_files(dir: &Path, n: i64, gh: &str) -> Option<Vec<String>> {
    let pr = n.to_string();
    let args = ["pr", "view", &pr, "--json", "files", "-q", ".files[].path"];
    let out = Command::new(gh).args(args).current_dir(dir).output().ok()?;
    let text = String::from_utf8(out.stdout)
        .ok()
        .filter(|_| out.status.success())?;
    let files: Vec<String> = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    (!files.is_empty()).then_some(files)
}

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
pub(crate) fn none_claimed(trunk: &str, pr: Option<i64>, uid: &str) -> String {
    let named = pr.map(|n| format!(" names (#{n})")).unwrap_or_default();
    // a PR is a change: `--no-change` is for a done with neither PR nor file
    let review = match pr {
        None => format!(
            "; a review that changed nothing: fael chunk done {uid} \"<result>\" --left none --no-change (MCP: no_change: true)"
        ),
        Some(_) => String::new(),
    };
    format!(
        "no diff claimed: no run branch and no commit on {trunk}{named} — the next brief prints no touched files; name the paths in the handoff{review}\n"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_missing_gh_gives_no_files() {
        let dir = std::env::temp_dir();
        assert_eq!(super::open_files(&dir, 8, "fael-no-such-gh-binary"), None);
    }
}
