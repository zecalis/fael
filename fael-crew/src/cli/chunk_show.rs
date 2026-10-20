//! `fael chunk show <uid|label> [--json]`: one chunk as the board sees it, with its hand-off,
//! the text its next start prints.

use crate::plan::db;
use crate::{Args, Repo};
use fael_crew::Store;
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
        .ok_or_else(|| missing(&s, &b.uid))?;
    if a.has("json") {
        let mut v = serde_json::to_value(&c).map_err(|e| e.to_string())?;
        v["brief"] = b.brief.into();
        v["said"] = b.said.into();
        v["notes"] = b.notes.into();
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
    for (head, texts) in [("owner said", &b.said), ("note", &b.notes)] {
        for t in texts {
            println!("{head}: {t}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// A chunk the board does not list: say why — its plan is parked or done (the
/// board only lists active plans), or the chunk itself is dropped — with the
/// retype that reads it. The `is not on the board` needle keeps its
/// `chunk_missing` code.
fn missing(s: &Store, uid: &str) -> String {
    match s.chunk_place(uid) {
        Ok(Some((state, plan, _))) if state == "dropped" => format!(
            "rejected: chunk {uid} is not on the board — it is dropped — \
            `fael plan export {plan}` lists its uid"
        ),
        Ok(Some((_, plan, plan_state))) if matches!(plan_state.as_str(), "parked" | "done") => {
            let unpark = if plan_state == "parked" {
                format!("`fael plan unpark {plan}` first, or ")
            } else {
                String::new()
            };
            format!(
                "rejected: chunk {uid} is not on the board — its plan {plan} is {plan_state} — \
                {unpark}`fael plan export {plan}` lists its uids"
            )
        }
        _ => format!("rejected: chunk {uid} is not on the board"),
    }
}

/// The forms an agent types for `chunk show` that name a plan and a label: `p/label`,
/// `p:label`, `p label`, `--uid X`. Each becomes `show <uid>` through `uid_in` (the plan
/// checks the label) or rejects naming the retype; anything else passes untouched.
pub(crate) fn natural(
    a: &Args,
    rest: &[String],
    r: &Repo,
) -> Result<Option<(Args, Vec<String>)>, String> {
    let words: Vec<&str> = rest.iter().map(String::as_str).collect();
    let (key, plan_label) = match (words.as_slice(), a.one("uid")) {
        (["show"], Some(u)) => (u, None),
        (["show", _], Some(_)) | (["show", _, _], Some(_)) => {
            return Err(
                "rejected: chunk show takes a uid or label, not both --uid and a name — \
                        retype `fael chunk show <uid>`"
                    .into(),
            );
        }
        (["show", k], None) if !a.has("plan") => match k.rsplit_once(['/', ':']) {
            Some((p, l)) if !p.is_empty() && !l.is_empty() => (String::new(), Some((p, l))),
            _ => return Ok(None),
        },
        (["show", p, l], None) if !a.has("plan") => {
            // two uids for the multi loop, not a plan and a label: `show <u1> <u2>`
            // would otherwise read as plan `u1` holding label `u2`
            if crate::core::looks_like_id(p) || crate::core::looks_like_id(l) {
                return Ok(None);
            }
            (String::new(), Some((*p, *l)))
        }
        _ => return Ok(None),
    };
    let uid = match plan_label {
        None => key,
        Some((p, l)) => Store::open(&db(r))?.uid_in(l, p).map_err(|e| {
            format!("{e}\nretype `fael chunk show --plan {p} {l}` with a plan and label from `fael board`")
        })?,
    };
    Ok(Some((a.without("uid"), vec!["show".into(), uid])))
}
