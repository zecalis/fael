//! `fael chunk done "<result>"` with no uid (plan fael-chunk-close): the chunk is the live
//! run of this worktree, and the PR comes from the run's branch when `--pr` is absent.

use crate::{Args, Repo};
use fael_crew::{Here, Store};
use std::path::Path;
use std::process::Command;

/// `chunk done "<result>"`: the uid and, without `--pr`/`--out`, the PR come from this worktree.
pub(crate) fn done(a: &Args, r: &Repo, handoff: &str, here: &Here) -> Result<(), String> {
    let st = Store::open(&crate::plan::db(r))?;
    let uid = uid(&st, here)?;
    let pr = if a.has("pr") || a.has("out") {
        None
    } else {
        pr_default(r, &st, &uid, here)
    };
    drop(st);
    crate::chunk_run::done(a, r, &uid, handoff, here, pr)
}

/// The uid of the one chunk this worktree holds, or why there is none.
pub(crate) fn uid(st: &Store, here: &Here) -> Result<String, String> {
    let held = st.held(here.worktree)?;
    match held.as_slice() {
        [] => Err(
            "rejected: no chunk run in this worktree — `fael board` lists the chunks; \
                   run `fael chunk done <uid> …` from the worktree that started it"
                .into(),
        ),
        [(uid, ..)] => Ok(uid.clone()),
        many => {
            let list: Vec<String> = many.iter().map(|(u, t, _)| format!("  {u} {t}")).collect();
            Err(format!(
                "rejected: more than one chunk runs in this worktree ({}) — name one: `fael chunk done <uid> …`\n{}",
                many.len(),
                list.join("\n")
            ))
        }
    }
}

/// The PR of `branch` through `gh`; None = gh missing, no PR, or no branch (done goes without).
pub(crate) fn pr_of(dir: &Path, branch: Option<&str>, gh: &str) -> Option<String> {
    let out = Command::new(gh)
        .args(["pr", "view", branch?, "--json", "number", "-q", ".number"])
        .current_dir(dir)
        .output()
        .ok()?;
    let n = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && n.parse::<i64>().is_ok()).then_some(n)
}

/// `--pr` taken from the run's branch once, said in one line either way.
pub(crate) fn pr_default(r: &Repo, st: &Store, uid: &str, here: &Here) -> Option<String> {
    let branch = st.run_branch(uid, None);
    let n = pr_of(&r.root, branch.as_deref().or(here.branch), "gh");
    match &n {
        Some(n) => println!("--pr {n} from the run's branch"),
        None => println!(
            "no PR found for the run's branch (gh missing or none open) — done without a PR"
        ),
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_absent_is_no_pr_not_a_failure() {
        assert_eq!(pr_of(Path::new("."), Some("b"), "gh-does-not-exist"), None);
        assert_eq!(pr_of(Path::new("."), None, "gh"), None);
    }
}
