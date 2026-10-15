//! `fael intake [<source>]` (plan fael-intake, decision 01M4TKVY): run each source's `list`
//! from `.fael/sources.json`, check every line, then write the run in one transaction. Fael
//! runs only `list` — it never sends or writes anything to the source.

use crate::plan::db;
use crate::{Args, Repo, repo};
use fael_core::plan::{BoardChunk, Listed, Source, Store, listed, sources};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

pub(crate) fn cmd(a: &Args, only: Option<&String>) -> Result<ExitCode, String> {
    a.only("intake", &[])?;
    let r = repo()?;
    let mut all = read(&r.root)?.ok_or(
        "rejected: no .fael/sources.json — the app's adapter names its `list` there (see `fael intake --help`)",
    )?;
    if let Some(n) = only {
        let names: Vec<String> = all.iter().map(|s| s.name.clone()).collect();
        all.retain(|s| &s.name == n);
        if all.is_empty() {
            return Err(format!(
                "rejected: no source {n} in .fael/sources.json — {}",
                names.join(", ")
            ));
        }
    }
    // every list checked before one row is written
    let mut runs: Vec<(Source, Vec<Listed>)> = Vec::new();
    for s in all {
        let out = list(&r, &s.list).map_err(|e| format!("{}: {e}", s.name))?;
        let l = listed(&out).map_err(|e| format!("{}: {e} — nothing written", s.name))?;
        runs.push((s, l));
    }
    let n: usize = runs.iter().map(|(_, l)| l.len()).sum();
    let now = fael_core::rfc3339(fael_core::now_ms());
    let lines = Store::open(&db(&r))?.intake(&runs, &now)?;
    crate::board::register(&r);
    for l in &lines {
        println!("{l}");
    }
    let changed = lines.iter().filter(|l| !l.starts_with("skipped ")).count();
    println!("{changed} change(s) from {n} ticket(s)");
    Ok(ExitCode::SUCCESS)
}

/// `<root>/.fael/sources.json`; None = the file is absent.
fn read(root: &Path) -> Result<Option<Vec<Source>>, String> {
    match std::fs::read_to_string(root.join(".fael/sources.json")) {
        Ok(s) => sources(&s).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!(".fael/sources.json: {e}")),
    }
}

/// `list` through the platform shell at the repo root, the caller's env; stdout is the JSONL.
fn list(r: &Repo, cmd: &str) -> Result<String, String> {
    let (sh, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let o = Command::new(sh)
        .args([flag, cmd])
        .current_dir(&r.root)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("list: {sh}: {e}"))?;
    match o.status.code() {
        Some(0) => String::from_utf8(o.stdout).map_err(|_| "list: stdout is not UTF-8".into()),
        // sh found no such command; cmd /C exits 1 and prints its own "not recognized" line
        Some(127) => Err(format!("list: command not found — {cmd}")),
        c => Err(format!(
            "list exited {} — nothing written",
            c.map_or("on a signal".into(), |c| c.to_string())
        )),
    }
}

/// `fael board --json`: each ticket's `open` url from its source; a source no longer in
/// `.fael/sources.json` (or a broken file) leaves it null — the board never fails on it.
pub(crate) fn open_urls(root: &Path, chunks: &mut [BoardChunk]) {
    if chunks.iter().all(|c| c.ticket.is_none()) {
        return;
    }
    let Ok(Some(all)) = read(root) else { return };
    for t in chunks.iter_mut().filter_map(|c| c.ticket.as_mut()) {
        t.open = all
            .iter()
            .find(|s| s.name == t.source)
            .map(|s| s.open.replace("{ref}", &t.what));
    }
}
