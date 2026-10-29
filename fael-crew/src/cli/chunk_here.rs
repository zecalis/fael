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
    // `--no-change` names no PR: the branch's own is not the work reviewed (issue 01M62XEX)
    let pr = if a.has("pr") || a.has("out") || a.has("no-change") {
        None
    } else {
        pr_default(r, &st, &uid, here)
    };
    drop(st);
    crate::chunk_run::done(a, r, &uid, handoff, here, pr)
}

/// The hand-off text: `-` is read from stdin (trailing newline trimmed, empty refused);
/// anything else is itself.
pub(crate) fn handoff_text(arg: &str) -> Result<String, String> {
    if arg != "-" {
        return Ok(arg.to_string());
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
        .map_err(|e| format!("rejected: hand-off `-`: cannot read stdin: {e}"))?;
    let text = text.trim_end_matches(['\n', '\r']).to_string();
    match text.trim().is_empty() {
        true => Err("rejected: hand-off `-` read an empty stdin — pipe the text in".into()),
        false => Ok(text),
    }
}

/// What a real `done` says once about features the agent may not know: the amend, and the
/// review verdict (an amend takes text only, so `--review` rides on this done).
pub(crate) fn done_hints(uid: &str, done: bool, reviewed: bool) -> String {
    // only a chunk in `done` (a PR) is amended; a `review` one re-records on its next done
    let mut s = String::new();
    if done {
        s = format!(
            "hand-off changed after this (rebase, conflict)? fael chunk done {uid} \"<new hand-off>\" amends it\n"
        );
    }
    if !reviewed {
        s.push_str("no --review given: it records the fresh review's verdict (clean|fixed|failed); pass it on this done — an amend takes text only\n");
    }
    s
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
/// `FAEL_GH_PR` stands in for gh's answer (tests only: no fake binary on PATH, cf. `FAEL_GH_JSON`).
pub(crate) fn pr_of(dir: &Path, branch: Option<&str>, gh: &str) -> Option<String> {
    let branch = branch?;
    if let Ok(fake) = std::env::var("FAEL_GH_PR") {
        return fake.trim().parse::<i64>().ok().map(|n| n.to_string());
    }
    let out = Command::new(gh)
        .args(["pr", "view", branch, "--json", "number", "-q", ".number"])
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
