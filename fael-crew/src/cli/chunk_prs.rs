//! `chunk done --pr 1503,1505`: work that landed over more than one PR (the first merged
//! before the review fix, the fix on a new branch). The last PR is the one the board follows
//! — the one still open — and the earlier ones ride in the hand-off the next brief prints,
//! so `run.pr` stays one number (issue 01M6BKHZH).

use fael_crew::Dodge;
use std::path::Path;

/// `raw` split on commas: the last is the run's PR, the others the earlier numbers (`12`,
/// `#12`, `owner/repo#12` or a pull URL each; a bad one is the one-PR reject).
pub(crate) fn split(raw: Option<String>) -> Result<(Option<String>, Vec<i64>), String> {
    let Some(raw) = raw else {
        return Ok((None, vec![]));
    };
    let mut parts: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let last = parts.pop().map(String::from);
    let earlier = parts
        .into_iter()
        .map(|p| crate::chunk_run::pr_number(Some(p)).map(|n| n.unwrap_or_default()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((last, earlier))
}

/// The hand-off with its earlier PRs named; unchanged when there are none.
pub(crate) fn with_earlier(handoff: &str, earlier: &[i64]) -> String {
    if earlier.is_empty() {
        return handoff.to_string();
    }
    let list: Vec<String> = earlier.iter().map(|n| format!("#{n}")).collect();
    format!("{handoff}\nearlier PRs: {}", list.join(", "))
}

/// A fact when `--pr n` is not the PR `gh` finds for the run's branch: the work may be
/// split over both. Silent without gh, a branch or a PR; never a refusal.
pub(crate) fn branch_note(dir: &Path, branch: Option<&str>, n: i64) -> Option<String> {
    let m: i64 = crate::chunk_here::pr_of(dir, branch, "gh")?.parse().ok()?;
    (m != n).then(|| {
        format!(
            "note: branch {} has PR #{m}, not #{n} — work split over both? name both: --pr {n},{m} (the last is the board's)\n",
            branch.unwrap_or_default()
        )
    })
}

/// Says [`branch_note`] when the caller named one PR with `--pr` (not the branch's default).
pub(crate) fn say_branch(named: bool, (dir, branch): (&Path, Option<&str>), pr: Option<i64>) {
    if let (true, Some(n)) = (named, pr) {
        print!("{}", branch_note(dir, branch, n).unwrap_or_default());
    }
}

/// Push-PR survival (l4): `chunk done --pr` against live partners' touched
/// files (board dodge data). Exact warns with the way out; prefix is ok when
/// the hand-off's `left:` names the partner, else asks for the ref or the
/// reason `left: none` stands. Warn only — never blocks done.
pub(crate) fn dodge_text(dodges: &[Dodge], handoff: &str) -> String {
    let mut out = String::new();
    for d in dodges {
        let files = d.files.join(", ");
        let seat = match (&d.worktree, &d.branch) {
            (Some(w), Some(b)) => format!(" in {w} on {b}"),
            (Some(w), None) => format!(" in {w}"),
            (None, Some(b)) => format!(" on {b}"),
            (None, None) => String::new(),
        };
        if d.class == fael_core::OverlapClass::Exact {
            out += &format!(
                "dodge: {files} also touched live by {} \"{}\"{seat} — land after {} (rebase order), or split the file, or follow {} (`fael chunk start <uid> --with {} --as follow`)\n",
                d.uid, d.title, d.uid, d.uid, d.uid
            );
        } else if crate::chunk_pr_files::left_names(handoff, &d.uid) {
            out += &format!(
                "note: {files} under live {} scope — linked via --left\n",
                d.uid
            );
        } else {
            out += &format!(
                "note: {files} under live {} \"{}\"{seat} scope — name it: --left {} or explain why left:none stands\n",
                d.uid, d.title, d.uid
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_pr_is_the_runs_and_the_rest_ride_in_the_handoff() {
        let (last, earlier) = split(Some("1503, #1504,1505".into())).unwrap();
        assert_eq!(last.as_deref(), Some("1505"));
        assert_eq!(earlier, [1503, 1504]);
        assert_eq!(
            split(Some("12".into())).unwrap(),
            (Some("12".into()), vec![])
        );
        assert!(split(Some("x,12".into())).is_err());
        assert_eq!(with_earlier("done", &[]), "done");
        assert_eq!(with_earlier("done", &[1503]), "done\nearlier PRs: #1503");
    }

    fn dodge(uid: &str, class: fael_core::OverlapClass, files: &[&str]) -> Dodge {
        Dodge {
            uid: uid.into(),
            title: "partner".into(),
            class,
            files: files.iter().map(|s| s.to_string()).collect(),
            branch: None,
            worktree: None,
        }
    }

    #[test]
    fn dodge_text_warns_exact_with_the_way_out() {
        let out = dodge_text(
            &[dodge("01P", fael_core::OverlapClass::Exact, &["src/a.rs"])],
            "did it\nleft: none",
        );
        assert!(
            out.contains("dodge: src/a.rs also touched live by 01P"),
            "{out}"
        );
        assert!(
            out.contains("rebase order") && out.contains("split") && out.contains("follow"),
            "{out}"
        );
    }

    #[test]
    fn dodge_text_prefix_needs_a_left_ref() {
        let d = dodge("01P456", fael_core::OverlapClass::Prefix, &["src/a.rs"]);
        let linked = dodge_text(std::slice::from_ref(&d), "did it\nleft: 01P4");
        assert!(linked.contains("linked via --left"), "{linked}");
        let bare = dodge_text(&[d], "did it\nleft: none");
        assert!(
            bare.contains("--left 01P456") && bare.contains("left:none stands"),
            "{bare}"
        );
    }
}
