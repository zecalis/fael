//! `fael chunk show <uid|label> [--json]`: one chunk as the board sees it, with its hand-off,
//! the text its next start prints and the asks it holds in and out.

use crate::plan::db;
use crate::{Args, Repo};
use fael_core::plan::Store;
use std::process::ExitCode;

/// `fael chunk show <uid|label> [--json]`: the board's chunk object plus the brief, owner
/// answers and notes its next start prints — one chunk, no filtering the whole board.
pub(crate) fn show(a: &Args, r: &Repo, key: &str) -> Result<ExitCode, String> {
    a.only("chunk show", &["json"])?;
    let s = Store::open(&db(r))?;
    let b = s.show(key)?;
    let (at, stale) = crate::board::clock();
    let c = s
        .board(&at[..10], &stale)?
        .chunks
        .into_iter()
        .find(|c| c.uid == b.uid)
        .ok_or(format!("rejected: chunk {} is not on the board", b.uid))?;
    let asks = s.asks(&b.uid)?;
    if a.has("json") {
        let mut v = serde_json::to_value(&c).map_err(|e| e.to_string())?;
        v["brief"] = b.brief.into();
        v["said"] = b.said.into();
        v["notes"] = b.notes.into();
        v["asks"] = serde_json::to_value(&asks).map_err(|e| e.to_string())?;
        let s = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        println!("{s}");
        return Ok(ExitCode::SUCCESS);
    }
    let label = c
        .label
        .as_deref()
        .map(|l| format!(" {l}"))
        .unwrap_or_default();
    println!("{}  {}{label}  {}  {}", c.uid, c.plan, c.state, c.title);
    if let Some(run) = &c.run {
        let seen = run.last_seen.as_deref().unwrap_or("never");
        let flag = if c.stalled { " · stalled" } else { "" };
        let wt = run.worktree.as_deref().unwrap_or("no worktree");
        println!(
            "run {} in {wt} since {} · last seen {seen}{flag}",
            run.id, run.started
        );
    }
    // the result first: an owner without the board app reads a done chunk here
    if let Some(h) = &c.handoff {
        println!("hand-off {}", h.at);
        for l in h.text.lines() {
            println!("< {l}");
        }
    }
    for x in &c.blocked_by {
        println!("after {}", x.what);
    }
    for l in b.brief.lines() {
        println!("> {l}");
    }
    for l in crate::chunk_ask::lines(&asks) {
        println!("{l}");
    }
    for (head, texts) in [("owner said", &b.said), ("note", &b.notes)] {
        for t in texts {
            println!("{head}: {t}");
        }
    }
    Ok(ExitCode::SUCCESS)
}
