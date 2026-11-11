//! pr-cross: done chunks whose PR is still open and touches this run's files.
//! At `done` and in `chunk status`, take chunks in state done whose PR is still
//! open (the board already knows PR state — the squash subject on the base) and
//! intersect their PR files with this run's files; one line per PR, at most 3.
//! Leave alone: live-run dodge text; no `gh` call when no done chunk has an open PR.

use fael_crew::Store;
use std::collections::HashSet;
use std::path::Path;

/// One open done PR meeting this run: uid, PR number, shared files.
struct Hit {
    uid: String,
    pr: i64,
    files: Vec<String>,
}

/// The lines `done` and `status` print for open done PRs meeting `mine`:
/// `also in PR #<n> (<uid>): <files>`, one per PR, at most 3, uid-ordered.
/// Empty when nothing meets. No `gh` call unless a done PR is still open.
pub(crate) fn open_lines(st: &Store, root: &Path, exclude_uid: &str, mine: &[String]) -> String {
    hits(st, root, exclude_uid, mine)
        .map(|hits| text_of(&hits))
        .unwrap_or_default()
}

/// Fail-open: any store or git miss reads as no open PRs, never a reject.
fn hits(st: &Store, root: &Path, exclude_uid: &str, mine: &[String]) -> Option<Vec<Hit>> {
    if mine.is_empty() {
        return Some(Vec::new());
    }
    let done = st.done_prs(exclude_uid).ok()?;
    if done.is_empty() {
        return Some(Vec::new());
    }
    let base = crate::git(root, &["rev-parse", "--abbrev-ref", "origin/HEAD"])?;
    let since = done.iter().map(|d| d.started.as_str()).min()?;
    let log = crate::git(
        root,
        &["log", "--format=%s", &format!("--since={since}"), &base],
    )
    .unwrap_or_default();
    let open: Vec<&fael_crew::DonePr> = done
        .iter()
        .filter(|d| !log.lines().any(|s| crate::board::prs::names_pr(s, d.pr)))
        .collect();
    if open.is_empty() {
        return Some(Vec::new());
    }
    let shared = fael_crew::shared(root).unwrap_or_default();
    let mine_set: HashSet<&str> = mine
        .iter()
        .filter(|f| !shared.iter().any(|s| fael_core::under(f, s)))
        .map(String::as_str)
        .collect();
    if mine_set.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for d in open {
        let pr_files = super::chunk_pr_files::pr_files(root, &base, d.pr, "gh")?;
        let mut hit: Vec<String> = pr_files
            .into_iter()
            .filter(|f| !shared.iter().any(|s| fael_core::under(f, s)))
            .filter(|f| mine_set.contains(f.as_str()))
            .collect();
        if hit.is_empty() {
            continue;
        }
        hit.sort();
        out.push(Hit {
            uid: d.uid.clone(),
            pr: d.pr,
            files: hit,
        });
    }
    out.sort_by(|a, b| a.uid.cmp(&b.uid));
    out.truncate(3);
    Some(out)
}

fn text_of(hits: &[Hit]) -> String {
    let mut o = String::new();
    for h in hits {
        o += &format!("also in PR #{} ({}): {}\n", h.pr, h.uid, h.files.join(", "));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::Hit;
    use super::text_of;

    #[test]
    fn lines_name_pr_uid_and_files_capped_at_three() {
        let hits: Vec<Hit> = (1..=4)
            .map(|n| Hit {
                uid: format!("01A{n}"),
                pr: n,
                files: vec!["src/x.rs".into()],
            })
            .collect();
        let out = text_of(&hits[..3]);
        assert!(out.contains("also in PR #1 (01A1): src/x.rs"), "{out}");
        assert_eq!(out.lines().count(), 3);
    }
}
