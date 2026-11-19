//! The git fact behind a `[Gone]`/`[PartGone]` file: the commit that last
//! removed it. Evidence for the reader, never a verdict — whether the row is
//! stale, moved or still true stays their call. One `git log` for the whole
//! set of missing files, however many rows name them (`fael/tests/top/git_budget.rs`).

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Longest subject printed; the sha finds the rest.
const SUBJECT_MAX: usize = 60;

/// What git says about one missing path, shortest wording first.
pub(super) fn why(root: &Path, files: &[&str]) -> HashMap<String, String> {
    let mut paths: Vec<&str> = files
        .iter()
        .copied()
        .filter(|f| !f.contains('\n'))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return HashMap::new();
    }
    // `--no-renames`: a move reads as the old path removed, with the commit
    // that moved it. Literal pathspecs: a name with `*` or `[` is a name.
    let mut args = vec![
        "--literal-pathspecs",
        "log",
        "--no-renames",
        "--name-status",
        "--format=%x01%h %s",
        "--",
    ];
    args.extend(&paths);
    // no git (or a repo with no commit): every file stays unexplained
    let out = crate::git(root, &args).unwrap_or_default();
    explain(&out, &paths)
}

/// Parse the log (newest commit first) into one phrase per path: the newest
/// commit that touched it decides — a `D` is the removal; anything else means
/// the file is still in history and only gone from the working tree.
fn explain(out: &str, paths: &[&str]) -> HashMap<String, String> {
    let want: HashSet<&str> = paths.iter().copied().collect();
    let mut said: HashMap<String, String> = HashMap::new();
    for block in out.split('\u{1}').skip(1) {
        let mut lines = block.lines();
        let head = lines.next().unwrap_or_default();
        for l in lines {
            let Some((st, path)) = l.split_once('\t') else {
                continue;
            };
            if !want.contains(path) || said.contains_key(path) {
                continue;
            }
            let how = if st == "D" {
                format!("removed in {}", short(head))
            } else {
                "deleted, not committed".to_string()
            };
            said.insert(path.to_string(), how);
        }
    }
    for p in paths {
        said.entry((*p).to_string())
            .or_insert_with(|| "never in git".to_string());
    }
    said
}

/// `abc1234 subject` → `abc1234 "subject"`, the subject cut at `SUBJECT_MAX`.
fn short(head: &str) -> String {
    let (sha, subject) = head.split_once(' ').unwrap_or((head, ""));
    let cut: String = subject.chars().take(SUBJECT_MAX).collect();
    let dots = if subject.chars().count() > SUBJECT_MAX {
        "…"
    } else {
        ""
    };
    format!("{sha} \"{cut}{dots}\"")
}

#[cfg(test)]
mod tests {
    use super::explain;

    #[test]
    fn the_newest_touch_decides_and_an_unseen_path_was_never_in_git() {
        let log = "\u{1}a1b2c3d remove c\n\nD\tsrc/c.rs\n\u{1}e4f5a6b add c and a\n\nA\tsrc/c.rs\nA\tsrc/a.rs\n";
        let got = explain(log, &["src/c.rs", "src/a.rs", "src/d.rs"]);
        assert_eq!(got["src/c.rs"], "removed in a1b2c3d \"remove c\"");
        assert_eq!(got["src/a.rs"], "deleted, not committed");
        assert_eq!(got["src/d.rs"], "never in git");
    }

    #[test]
    fn a_long_subject_is_cut() {
        let log = format!("\u{1}a1b2c3d {}\n\nD\tx\n", "w".repeat(80));
        let got = explain(&log, &["x"]);
        assert!(got["x"].ends_with("…\""), "{}", got["x"]);
        assert!(got["x"].chars().count() < 90, "{}", got["x"]);
    }
}
