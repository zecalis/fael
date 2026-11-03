//! `fael chunk status <uid> [--json]`: both sides of the joint run at once (f6).
//! DB from the store, git live in the CLI — the same split as board conflicts
//! (`merge-tree`, CLI fills). Read-only: no state change.

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
}

/// `fael chunk status <uid>`: each side's branch/base/ahead + unstaged.
pub(crate) fn status(a: &Args, r: &Repo, key: &str) -> Result<ExitCode, String> {
    a.only("chunk status", &["json"])?;
    let s = Store::open(&db(r))?;
    let uid = s.uid_any(key)?;
    let sides = s.status(&uid)?;
    let filled: Vec<Filled> = sides.into_iter().map(|s| fill(&s)).collect();
    if a.has("json") {
        let v: Vec<serde_json::Value> = filled.iter().map(json_of).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
        );
        return Ok(ExitCode::SUCCESS);
    }
    println!("{}", text_of(&filled));
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
        o += &format!("  ahead {} commit(s) {}..HEAD:\n", f.ahead.len(), base);
        if f.ahead.is_empty() {
            o += "    (none)\n";
        }
        for c in &f.ahead {
            o += &format!("    {c}\n");
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

/// The files an ahead log filters by: live `touched`, else declared scope.
fn effective(s: &StatusSide) -> Vec<String> {
    if s.touched.is_empty() {
        s.scope.clone()
    } else {
        s.touched.clone()
    }
}

fn short(sha: Option<&str>) -> Option<String> {
    sha.map(|s| s.chars().take(8).collect())
}

/// DB from the store, git live here: branch, `base..HEAD` filtered by the
/// side's files, `status --porcelain`. Fail-open: no worktree or git error
/// reads empty, never a reject.
fn fill(s: &StatusSide) -> Filled {
    let Some(wt) = s.worktree.as_deref() else {
        return Filled {
            side: s.clone(),
            branch: s.branch.clone().unwrap_or_else(|| "-".into()),
            ahead: Vec::new(),
            unstaged: Vec::new(),
        };
    };
    let dir = Path::new(wt);
    let branch = crate::git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|b| b != "HEAD")
        .or_else(|| s.branch.clone())
        .unwrap_or_else(|| "-".into());
    let ahead = ahead_of(dir, s.base.as_deref(), &effective(s));
    let unstaged = crate::git(dir, &["status", "--porcelain"])
        .map(|o| o.lines().map(String::from).collect())
        .unwrap_or_default();
    Filled {
        side: s.clone(),
        branch,
        ahead,
        unstaged,
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
    use super::ahead_of;
    use std::path::Path;

    #[test]
    fn no_base_reads_no_ahead() {
        assert!(ahead_of(Path::new("/nope"), None, &[]).is_empty());
    }
}
