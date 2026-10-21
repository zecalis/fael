//! `fael sweep --input` (sweep skill's one read): this repo only — the untriaged
//! issues with bodies, the commits naming each one, the scopes in flight, and
//! the drifted rows with the file that moved most. One command, one `git log`
//! per signal, so a sweep stops re-reading the whole board and re-finding
//! every id.

use crate::{Args, Repo, core};
use serde::Serialize;
use std::collections::HashMap;
use std::process::ExitCode;

#[derive(Serialize)]
struct Input {
    v: u32,
    root: String,
    issues: Vec<Issue>,
    active_scopes: Vec<Scope>,
    claimed: Vec<Claimed>,
    drifted: Vec<Drifted>,
}

#[derive(Serialize)]
struct Issue {
    id: String,
    title: Option<String>,
    text: String,
    key: Option<String>,
    files: Vec<String>,
    at: String,
    by: String,
    from: Option<String>,
    to: Option<String>,
    commits: Vec<String>,
}

#[derive(Serialize)]
struct Scope {
    uid: String,
    state: String,
    scope: Vec<String>,
}

#[derive(Serialize)]
struct Claimed {
    id: String,
    held: String,
    files: Vec<String>,
}

#[derive(Serialize)]
struct Drifted {
    id: String,
    kind: String,
    title: Option<String>,
    text: String,
    key: Option<String>,
    files: Vec<String>,
    at: String,
    by: String,
    from: Option<String>,
    top_file: String,
    top_count: usize,
    total: usize,
}

pub(crate) fn cmd(a: &Args) -> Result<ExitCode, String> {
    a.only("sweep", &["input", "json"])?;
    if !a.has("input") {
        return Err(
            "rejected: --input is required — fael sweep --input is the sweep's one read".into(),
        );
    }
    let r = crate::repo()?;
    let log = crate::read(&r);
    let open = core::find(&log, &core::Filter::default());
    let input = Input {
        v: 1,
        root: r.root.to_string_lossy().into_owned(),
        issues: untriaged(&r, &log, &open),
        active_scopes: scopes(&r),
        claimed: claimed(&open),
        drifted: drifted(&log, &r),
    };
    println!(
        "{}",
        serde_json::to_string(&input).map_err(|e| e.to_string())?
    );
    Ok(ExitCode::SUCCESS)
}

/// Untriaged open issues with bodies and naming commits — what the sweep judges.
fn untriaged(r: &Repo, log: &core::Log, open: &[&core::Row]) -> Vec<Issue> {
    let fixers = fixers(r);
    let mut issues: Vec<&core::Row> = open.iter().copied().filter(|x| x.kind == "issue").collect();
    issues.sort_by_key(|x| x.id.clone());
    // a re-filed issue (superseded) links through its newest id — like the board
    let pred: HashMap<&str, &str> = core::successors(log)
        .into_iter()
        .map(|(old, new)| (new, old))
        .collect();
    let linked = |id: &str| {
        let mut id = id;
        for _ in 0..=pred.len() {
            if fixers.contains_key(id) {
                return true;
            }
            match pred.get(id).copied() {
                Some(next) => id = next,
                None => return false,
            }
        }
        false
    };
    let live: Vec<&&core::Row> = issues
        .iter()
        .filter(|x| x.held().is_none() && !linked(&x.id))
        .collect();
    let ids: Vec<String> = live.iter().map(|x| x.id.clone()).collect();
    let commits = issue_commits(r, &ids);
    live.iter()
        .map(|x| Issue {
            id: x.id.clone(),
            title: x.title.clone(),
            text: x.text.clone(),
            key: x.key.clone(),
            files: x.files.clone(),
            at: x.ts.clone(),
            by: x.by.clone(),
            from: x.from.clone(),
            to: x.to.clone(),
            commits: commits.get(&x.id).cloned().unwrap_or_default(),
        })
        .collect()
}

/// Open issues somebody holds (`fael claim`) — claimed work a new chunk avoids.
fn claimed(open: &[&core::Row]) -> Vec<Claimed> {
    open.iter()
        .filter(|x| x.kind == "issue")
        .filter_map(|x| {
            x.held().map(|h| Claimed {
                id: x.id.clone(),
                held: h.to_string(),
                files: x.files.clone(),
            })
        })
        .collect()
}

/// Drifted decisions and notes with the file that moved most and its count.
fn drifted(log: &core::Log, r: &Repo) -> Vec<Drifted> {
    fael_core::drift::detail(log, &r.root)
        .into_iter()
        .filter(|(x, _, _, _)| matches!(x.kind.as_str(), "decision" | "note"))
        .map(|(x, top_file, top_count, total)| Drifted {
            id: x.id.clone(),
            kind: x.kind.clone(),
            title: x.title.clone(),
            text: x.text.clone(),
            key: x.key.clone(),
            files: x.files.clone(),
            at: x.ts.clone(),
            by: x.by.clone(),
            from: x.from.clone(),
            top_file,
            top_count,
            total,
        })
        .collect()
}

fn fixers(r: &Repo) -> HashMap<String, fael_crew::Fixer> {
    let path = crate::plan::db(r);
    if !path.exists() {
        return HashMap::new();
    }
    fael_crew::Store::open(&path)
        .map(|s| s.fixers().unwrap_or_default())
        .unwrap_or_default()
}

/// Scopes in flight: open/running/waiting chunks with a scope — what a new chunk
/// must not collide with. Lines only, no briefs or handoffs.
fn scopes(r: &Repo) -> Vec<Scope> {
    let path = crate::plan::db(r);
    if !path.exists() {
        return vec![];
    }
    let Ok(s) = fael_crew::Store::open(&path) else {
        return vec![];
    };
    let (at, stale) = crate::board::clock();
    let Ok(b) = s.board(&at[..10], &stale) else {
        return vec![];
    };
    b.chunks
        .into_iter()
        .filter(|c| matches!(c.state.as_str(), "open" | "running" | "waiting"))
        .filter(|c| !c.scope.is_empty())
        .map(|c| Scope {
            uid: c.uid,
            state: c.state,
            scope: c.scope,
        })
        .collect()
}

/// One `git log` for every id: `--grep=a --grep=b …` ORs, then each commit maps
/// back by substring. `sha subject` per commit, newest first, capped per issue.
fn issue_commits(r: &Repo, ids: &[String]) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    if ids.is_empty() {
        return out;
    }
    let mut full = vec!["log".to_string(), "--format=%x01%H %s".to_string()];
    full.extend(ids.iter().map(|id| format!("--grep={id}")));
    full.push("HEAD".into());
    let args: Vec<&str> = full.iter().map(String::as_str).collect();
    let Some(txt) = crate::git(&r.root, &args) else {
        return out;
    };
    for block in txt.split('\x01') {
        let line = block.trim();
        if line.is_empty() {
            continue;
        }
        for id in ids {
            if line.contains(id) {
                let list = out.entry(id.clone()).or_default();
                if list.len() < 10 && !list.iter().any(|l| l == line) {
                    list.push(line.to_string());
                }
            }
        }
    }
    out
}
