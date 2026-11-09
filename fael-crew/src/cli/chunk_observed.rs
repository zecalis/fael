//! `chunk status` observed files (issue 01M6Z008): the worktree's real diff, not its
//! declared scope. `touched()` is the committed trunk diff since the run's start
//! (`chunk_run`, with the stacked-lane `since` cut); porcelain adds the uncommitted
//! half. Agent-reported `run.touched` is never filled from git here — observed is
//! computed, shown and dropped, only `done`/`run end` claim a diff into the chunk.

use fael_crew::Store;
use std::path::Path;

/// Cap: a worktree diff bigger than this is still one screen of signal, never a dump.
pub(crate) const CAP: usize = 500;

/// The worktree's observed files: committed trunk diff (since the run's start) plus
/// uncommitted porcelain paths, deduped, capped. Fail-open: no worktree diff reads empty.
pub(crate) fn of(dir: &Path, st: &Store, uid: &str, branch: Option<&str>) -> Vec<String> {
    let committed = super::chunk_run::touched(dir, st, uid, branch).unwrap_or_default();
    let porcelain = crate::git(dir, &["status", "--porcelain"])
        .map(|o| parse_porcelain(&o))
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for f in committed.into_iter().chain(porcelain) {
        if seen.insert(f.clone()) {
            out.push(f);
        }
        if out.len() >= CAP {
            break;
        }
    }
    out
}

/// `git status --porcelain` v1 lines to repo-relative paths. Rename/copy lines
/// (`R  old -> new`) keep the new side; quoted paths lose their quotes; short or
/// empty lines are skipped.
pub(crate) fn parse_porcelain(out: &str) -> Vec<String> {
    let mut v = Vec::new();
    for line in out.lines() {
        if line.starts_with("## ") || line.len() < 4 {
            continue;
        }
        let mut p = line[3..].trim();
        if let Some((_, new)) = p.split_once(" -> ") {
            p = new.trim();
        }
        let p = p.trim_matches('"').trim();
        if !p.is_empty() {
            v.push(p.to_string());
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::parse_porcelain;

    #[test]
    fn porcelain_keeps_paths_skips_noise_and_rename_old_side() {
        assert!(parse_porcelain("").is_empty());
        assert!(parse_porcelain("## main...origin/main").is_empty());
        let got = parse_porcelain(" M src/a.rs\nA  new/b.txt\n?? stray.txt\nR  old.rs -> new.rs\n");
        assert_eq!(got, vec!["src/a.rs", "new/b.txt", "stray.txt", "new.rs"]);
    }

    #[test]
    fn porcelain_strips_quotes_and_trims() {
        let got = parse_porcelain(" M \"sp ace.rs\"\n?? \"q.rs\"\n");
        assert_eq!(got, vec!["sp ace.rs", "q.rs"]);
    }

    #[test]
    fn short_lines_are_skipped_never_panic() {
        assert!(parse_porcelain("M\n M\n??\n").is_empty());
    }
}
