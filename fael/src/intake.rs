//! `fael intake [<source>]` (plan ledger-intake, decisions 01M4V3AD, 01M5CY01): tickets an
//! app's adapter lists become ledger rows. Runs each source's `list` from `.fael/sources.json`,
//! checks every line and every key, then writes through the same path as `fael add` /
//! `fael close`. Fael never writes to the source.
//!
//! Row per ticket round: bug|report|question → issue, idea → note; key
//! `ticket:<source>:<ref>` (`-r<N>` on reopen N), files = that key (an anchor), by `intake`.
//! One open row per ticket across its rounds: dedup = any row on the key, closed ones too (a
//! ticket an agent closed is not reopened by the source still listing it); a reopen closes
//! the earlier round's open row; copies written on the same key by two machines before a
//! sync are closed on the next run, smallest id wins. The source closing a ticket closes its
//! open row.

mod list;
#[cfg(test)]
mod tests;

use crate::{Args, Repo, core, repo, write};
use list::{Listed, Source, listed, sources};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

const BY: &str = "intake";

/// One write of a run, decided before any is made.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Add {
        what: String,
        kind: &'static str,
        key: String,
        title: String,
        text: String,
    },
    Close {
        what: String,
        id: String,
        why: String,
    },
}

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
    let mut runs = Vec::new();
    for s in all {
        let out = run_list(&r, &s.list).map_err(|e| format!("{}: {e}", s.name))?;
        let l = listed(&out).map_err(|e| format!("{}: {e} — nothing written", s.name))?;
        runs.push((s, l));
    }
    let n: usize = runs.iter().map(|(_, l)| l.len()).sum();
    let steps = plan(&crate::read(&r), &runs).map_err(|e| format!("{e} — nothing written"))?;
    let (mut failed, mut changed) = (0, 0);
    for step in &steps {
        match apply(&r, step) {
            Ok(line) => {
                changed += 1;
                println!("{line}");
            }
            Err(e) => {
                failed += 1;
                println!("rejected: {}", e.trim_start_matches("rejected: "));
            }
        }
    }
    println!("{changed} change(s) from {n} ticket(s)");
    if failed > 0 {
        return Err(format!(
            "rejected: {failed} of {} rows rejected — the rest saved",
            steps.len()
        ));
    }
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
fn run_list(r: &Repo, cmd: &str) -> Result<String, String> {
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

/// `ticket:<source>:<lower ref>`, `-r<N>` for reopen round N. Keys are lowercase.
fn key(s: &Source, t: &Listed) -> String {
    let base = format!("ticket:{}:{}", s.name, t.r#ref.to_lowercase());
    match t.reopen {
        0 => base,
        n => format!("{base}-r{n}"),
    }
}

fn row_kind(t: &Listed) -> Result<&'static str, String> {
    match t.kind.as_str() {
        "bug" | "report" | "question" => Ok("issue"),
        "idea" => Ok("note"),
        k => Err(format!(
            "ticket {}: kind {k:?} has no row — bug, report, question or idea",
            t.r#ref
        )),
    }
}

/// What every run writes, from the checked lists and the ledger as it stands. Err = a bad
/// ticket (kind, key): the caller writes nothing. One rule per listed ticket: at most one
/// open row across its rounds, and the smallest id on the current round survives, so every
/// machine picks the same row after a sync.
fn plan(log: &core::Log, runs: &[(Source, Vec<Listed>)]) -> Result<Vec<Step>, String> {
    let open = core::find(log, &core::Filter::default());
    let mut steps = Vec::new();
    for (s, list) in runs {
        for t in list {
            let k = key(s, t);
            core::valid_key(&k)?;
            let kind = row_kind(t)?;
            let base = format!("ticket:{}:{}", s.name, t.r#ref.to_lowercase());
            let mut survivor: Option<&str> = None;
            let mut shut = Vec::new();
            for row in &log.rows {
                let Some(rk) = row.key.as_deref().and_then(|x| x.strip_prefix(&base)) else {
                    continue;
                };
                let round = match rk.strip_prefix("-r").map(str::parse::<u32>) {
                    None if rk.is_empty() => 0,
                    Some(Ok(n)) => n,
                    _ => continue,
                };
                if round == t.reopen && survivor.is_none_or(|v| row.id.as_str() < v) {
                    survivor = Some(&row.id);
                }
                if !open.iter().any(|o| o.id == row.id) {
                    continue;
                }
                shut.push((round, row.id.as_str()));
            }
            for (round, id) in shut {
                let why = match (round.cmp(&t.reopen), &t.closed) {
                    (std::cmp::Ordering::Less, _) => {
                        format!("reopened at source as round {}", t.reopen)
                    }
                    (std::cmp::Ordering::Equal, Some(w)) => format!("closed at source: {w}"),
                    (std::cmp::Ordering::Equal, None) if survivor != Some(id) => {
                        format!("duplicate of {}", survivor.unwrap_or_default())
                    }
                    _ => continue,
                };
                // the same ticket listed twice closes a row once
                if !steps
                    .iter()
                    .any(|s| matches!(s, Step::Close { id: c, .. } if c == id))
                {
                    steps.push(Step::Close {
                        what: t.r#ref.clone(),
                        id: id.into(),
                        why,
                    });
                }
            }
            let added = steps
                .iter()
                .any(|s| matches!(s, Step::Add { key, .. } if *key == k));
            if t.closed.is_none() && survivor.is_none() && !added {
                steps.push(Step::Add {
                    what: t.r#ref.clone(),
                    kind,
                    title: t.title.clone(),
                    text: text(log, &steps, s, t, &k),
                    key: k,
                });
            }
        }
    }
    Ok(steps)
}

/// The row's body: where the ticket lives, and for a reopened round where the one before
/// stopped (its close reason, else that it is still open).
fn text(log: &core::Log, steps: &[Step], s: &Source, t: &Listed, k: &str) -> String {
    let fill = |c: &str| c.replace("{ref}", &t.r#ref);
    let mut out = format!("Ticket {} from {} ({}).", t.r#ref, s.name, t.kind);
    if let Some(c) = &s.read {
        out += &format!(" Read: {}.", fill(c));
    }
    if let Some(c) = &s.open {
        out += &format!(" Open: {}.", fill(c));
    }
    if t.reopen > 0 {
        let prev = key(
            s,
            &Listed {
                reopen: t.reopen - 1,
                ..t.clone()
            },
        );
        let ids: Vec<&str> = log
            .rows
            .iter()
            .filter(|r| r.key.as_deref() == Some(&prev))
            .map(|r| r.id.as_str())
            .collect();
        // a round this run closes as reopened has no close on record yet
        let stop = log
            .closes
            .iter()
            .rev()
            .find(|c| c.reference.as_deref().is_some_and(|x| ids.contains(&x)))
            .map(|c| format!("closed: {}", c.text))
            .or_else(|| {
                steps
                    .iter()
                    .any(|s| matches!(s, Step::Close { id, .. } if ids.contains(&id.as_str())))
                    .then(|| "closed as reopened".to_string())
            })
            .unwrap_or("no close on record".into());
        let base = k.rsplit_once("-r").map_or(k, |(b, _)| b);
        out += &format!(
            " Reopened (round {}); round {} stopped — {stop}. All rounds: fael find --key '{base}*' --all.",
            t.reopen,
            t.reopen - 1
        );
    }
    out
}

fn apply(r: &Repo, step: &Step) -> Result<String, String> {
    match step {
        Step::Add {
            what,
            kind,
            key,
            title,
            text,
        } => {
            let opts = write::AddOpts {
                key: Some(key.clone()),
                title: Some(title.clone()),
                to: None,
                from: None,
                revisit: None,
                urgent: core::Urgent::Unset,
                supersedes: None,
                force: false,
                by: Some(BY.into()),
            };
            let (row, _, warns) = write::add_row(r, kind, text, std::slice::from_ref(key), opts)?;
            // a deterministic key is never "new" news to the caller
            for w in warns.iter().filter(|w| !w.contains("new key")) {
                eprintln!("{w}");
            }
            Ok(format!("added {what} → {}", row.id))
        }
        Step::Close { what, id, why } => {
            let (row, _, warns) = write::close_row_as(r, id, why, Some(BY), true)?;
            warns.iter().for_each(|w| eprintln!("{w}"));
            Ok(format!("closed {what} → {}", row.id))
        }
    }
}
