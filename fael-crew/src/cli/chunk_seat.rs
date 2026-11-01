//! Where a `fael chunk` call runs. A shell call runs where the agent is; an MCP server was
//! launched somewhere else, so its call names the directory (`--cwd`, the MCP `cwd`) or —
//! for the verbs that answer to a run — carries `--seat`: no directory known, take the one a
//! recorded live run says it sits in. The run is a fact fael holds, never a guess, and the
//! call says which seat it used (issue 01M6BK561).

use crate::plan::db;
use crate::{Args, Repo, repo, repo_at};
use fael_crew::Store;
use std::path::Path;

/// The verbs `--seat` serves: the ones fenced to the worktree of the chunk's latest run.
const SEATED: &[&str] = &["done", "wait", "after"];

/// The repo this call acts on: `--cwd <dir>`, else the process dir — moved to the live
/// live run's seat when `--seat` is given and `rest` is `<verb> <uid> …`.
pub(crate) fn repo_for(a: &Args, rest: &[String]) -> Result<Repo, String> {
    if let Some(d) = a.one("cwd") {
        return repo_at(Path::new(&d));
    }
    let r = repo()?;
    let [verb, uid, _, ..] = rest else {
        return Ok(r);
    };
    if !a.has("seat") || !SEATED.contains(&verb.as_str()) || a.has("run") {
        return Ok(r);
    }
    // no live run (an amend after done, a chunk never started): the process dir, as ever
    let Some((run, seat)) = Store::open(&db(&r))?.seat(uid)? else {
        return Ok(r);
    };
    if !Path::new(&seat).is_dir() {
        return Err(format!(
            "rejected: run {run}'s seat {seat} is gone — if {run} is yours, try the same command with --run {run}"
        ));
    }
    println!("seat: {seat} (run {run}) — no cwd given");
    repo_at(Path::new(&seat))
}
