//! `fael chunk status <uid> [--json]`: both sides of the joint run at once (f6), plus real
//! conflicts with other live runs (`chunk_conflicts`).
//! DB from the store, git live in the CLI — the same split as board conflicts
//! (`merge-tree`, CLI fills). Read-only: no state change.
//!
//! Diff-aware (issue 01M6Z008): besides the declared `files:` line, each side shows its
//! `observed` worktree diff (committed trunk diff since the run's start plus uncommitted
//! porcelain paths, `chunk_observed`) and each observed path outside its declared scope —
//! the same fact `done`/`run end` claim, but before landing. The edit hook stays
//! scope-only (no git spawn); this command is the early warning.

use super::chunk_conflicts as conflicts;
use super::chunk_drift::Moved;
use crate::plan::db;
use crate::{Args, Repo};
use fael_crew::{StatusSide, Store};
use std::path::Path;
use std::process::ExitCode;

/// One side with its live git reads filled in.
struct Filled {
    side: StatusSide,
    branch: String,
    ahead: Vec<String>,
    unstaged: Vec<String>,
    /// a running side: what the default branch did since its base
    moved: Option<Moved>,
    /// the worktree's real diff, capped (`chunk_observed`)
    observed: Vec<String>,
    /// each observed path outside the side's declared scope, with the open chunk holding it
    outside: Vec<(String, Option<String>)>,
}

/// `fael chunk status <uid>`: each side's branch/base/ahead + unstaged + observed overlap.
pub(crate) fn status(a: &Args, r: &Repo, key: &str) -> Result<ExitCode, String> {
    a.only("chunk status", &["json"])?;
    let st = Store::open(&db(r))?;
    let uid = st.uid_any(key)?;
    let sides = st.status(&uid)?;
    let shared = fael_crew::shared(&r.root).unwrap_or_default();
    let filled: Vec<Filled> = sides
        .into_iter()
        .map(|s| {
            let mut f = fill(&st, &shared, &s);
            f.moved = super::chunk_drift::of(&st, r, &s.uid).filter(|_| s.state == "running");
            f
        })
        .collect();
    // fail open: no worktree or a store error reads as no neighbours, never a reject
    let hits = filled
        .iter()
        .find(|f| f.side.uid == uid)
        .and_then(|f| f.side.worktree.as_deref())
        .map(|wt| conflicts::check(Path::new(wt), st.neighbours(&uid).unwrap_or_default()))
        .unwrap_or_default();
    if a.has("json") {
        let mut v: Vec<serde_json::Value> = filled.iter().map(json_of).collect();
        if let Some(m) = v.iter_mut().find(|x| x["uid"] == uid.as_str()) {
            m["conflicts"] = conflicts::json_of(&hits)["conflicts"].take();
            m["unchecked"] = conflicts::json_of(&hits)["unchecked"].take();
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
        );
        return Ok(ExitCode::SUCCESS);
    }
    print!("{}", text_of(&filled));
    print!("{}", conflicts::text_of(&hits));
    Ok(ExitCode::SUCCESS)
}

fn json_of(f: &Filled) -> serde_json::Value {
    serde_json::json!({
        "uid": f.side.uid,
        "title": f.side.title,
        "state": f.side.state,
        "run": f.side.run,
        "joint": f.side.joint,
        "partner": f.side.partner,
        "partners": f.side.partners,
        "branch": f.branch,
        "base": f.side.base,
        "worktree": f.side.worktree,
        "touched": f.side.touched,
        "scope": f.side.scope,
        "ahead": f.ahead,
        "unstaged": f.unstaged,
        "observed": f.observed,
        "outside": f.outside.iter().map(|(file, owner)| serde_json::json!({"file": file, "owner": owner})).collect::<Vec<_>>(),
        "drift": f.moved,
    })
}

fn text_of(all: &[Filled]) -> String {
    let mut o = String::new();
    o += &format!("status {} side(s)\n", all.len());
    for f in all {
        let run = f.side.run.as_deref().unwrap_or("-");
        let joint = match (&f.side.joint, &f.side.partner) {
            (Some(j), Some(p)) => format!(" {j} with {p}"),
            (Some(j), None) => format!(" {j}"),
            _ => String::new(),
        };
        o += &format!(
            "side {} \"{}\" {} run {run}{joint}\n",
            f.side.uid, f.side.title, f.side.state
        );
        let wt = f.side.worktree.as_deref().unwrap_or("no worktree");
        let base = short(f.side.base.as_deref()).unwrap_or_else(|| "-".into());
        o += &format!("  branch {} base {base} worktree {wt}\n", f.branch);
        let eff = effective(&f.side);
        o += &format!("  files: {}\n", eff.join(", "));
        o += &format!("  observed: {}\n", none(&f.observed));
        for (file, owner) in &f.outside {
            match owner {
                Some(name) => {
                    o +=
                        &format!("  outside declared scope: {file} (overlaps open chunk {name})\n");
                }
                None => o += &format!("  outside declared scope: {file}\n"),
            }
        }
        let free: Vec<&str> = f
            .outside
            .iter()
            .filter(|(_, o)| o.is_none())
            .map(|(x, _)| x.as_str())
            .collect();
        if !free.is_empty() {
            let plus: Vec<String> = free.iter().map(|p| format!("+{}", p.trim())).collect();
            o += &format!(
                "  to cover them: fael chunk edit {} --scope {}\n",
                f.side.uid,
                plus.join(" --scope "),
            );
        }
        o += &format!("  ahead {} commit(s) {}..HEAD:\n", f.ahead.len(), base);
        if f.ahead.is_empty() {
            o += "    (none)\n";
        }
        for c in &f.ahead {
            o += &format!("    {c}\n");
        }
        if let Some(m) = &f.moved {
            o += &m.text(&base, &f.side.uid);
        }
        o += "  unstaged:\n";
        if f.unstaged.is_empty() {
            o += "    (clean)\n";
        }
        for u in &f.unstaged {
            o += &format!("    {u}\n");
        }
    }
    o
}

fn none(files: &[String]) -> String {
    if files.is_empty() {
        "(none)".to_string()
    } else {
        files.join(", ")
    }
}

/// The files an ahead log filters by: declared scope, plus anything the worktree
/// really touched — a commit outside the scope is still this side's work.
fn effective(s: &StatusSide) -> Vec<String> {
    if s.touched.is_empty() {
        s.scope.clone()
    } else {
        s.touched.clone()
    }
}

/// Union keeping order: declared intent first, observed facts after, no dupes.
fn union_files(a: &[String], b: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for f in a.iter().chain(b) {
        if seen.insert(f.clone()) {
            out.push(f.clone());
        }
    }
    out
}

fn short(sha: Option<&str>) -> Option<String> {
    sha.map(|s| s.chars().take(8).collect())
}

/// DB from the store, git live here: branch, `base..HEAD` filtered by the
/// side's files, `status --porcelain`, plus the observed diff and its outside
/// check. Fail-open: no worktree or git error reads empty, never a reject.
fn fill(st: &Store, shared: &[String], s: &StatusSide) -> Filled {
    let Some(wt) = s.worktree.as_deref() else {
        return Filled {
            side: s.clone(),
            branch: s.branch.clone().unwrap_or_else(|| "-".into()),
            ahead: Vec::new(),
            unstaged: Vec::new(),
            moved: None,
            observed: Vec::new(),
            outside: Vec::new(),
        };
    };
    let dir = Path::new(wt);
    let branch = crate::git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|b| b != "HEAD")
        .or_else(|| s.branch.clone())
        .unwrap_or_else(|| "-".into());
    let observed = super::chunk_observed::of(
        dir,
        st,
        &s.uid,
        if branch == "-" {
            s.branch.as_deref()
        } else {
            Some(branch.as_str())
        },
    );
    let ahead = ahead_of(
        dir,
        s.base.as_deref(),
        &union_files(&effective(s), &observed),
    );
    let unstaged = crate::git(dir, &["status", "--porcelain"])
        .map(|o| o.lines().map(String::from).collect())
        .unwrap_or_default();
    let outside = st
        .outside(&s.uid, &observed, shared)
        .map(|(_, o)| o)
        .unwrap_or_default();
    Filled {
        side: s.clone(),
        branch,
        ahead,
        unstaged,
        moved: None,
        observed,
        outside,
    }
}

fn ahead_of(dir: &Path, base: Option<&str>, files: &[String]) -> Vec<String> {
    let Some(base) = base else {
        return Vec::new();
    };
    let range = format!("{base}..HEAD");
    let mut args: Vec<&str> = vec!["log", "--oneline", &range, "--"];
    let owned: Vec<String> = files.to_vec();
    if !owned.is_empty() {
        args.extend(owned.iter().map(String::as_str));
    }
    crate::git(dir, &args)
        .map(|o| o.lines().map(String::from).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{ahead_of, union_files};
    use std::path::Path;

    #[test]
    fn no_base_reads_no_ahead() {
        assert!(ahead_of(Path::new("/nope"), None, &[]).is_empty());
    }

    #[test]
    fn union_keeps_order_without_dupes() {
        let a = vec!["x".to_string()];
        let b = vec!["x".to_string(), "y".to_string()];
        assert_eq!(union_files(&a, &b), vec!["x".to_string(), "y".to_string()]);
    }
}
