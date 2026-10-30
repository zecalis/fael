//! `fael chunk start <uid> [--run R] [--client C] [--force]` (SPEC §1 run rules, §4 pair):
//! claim the chunk and every chunk paired with it in one `BEGIN IMMEDIATE` — all or none —
//! insert one run row each under the same R, and hand back what the brief prints.
//! One worktree is one seat: a live run there is an agent that left, so a start ends it
//! (`--force` is only for a live run in another worktree).

use super::State;
use super::agent::unwait;
use super::chunk::{Here, end_live, row, set, tx};
use super::holder::{
    end_caller_seat, end_here, holder, holder_detail, holder_worktree, members, ready_paired,
};
use super::joint::{self, Joint};
use super::ready::Ready;
use super::section::Row as Section;
use super::start_brief::brief;
use super::store::{Store, err};
use rusqlite::{OptionalExtension, Transaction, params};

#[derive(Debug, Default, Clone)]
pub struct Start {
    /// R: the launcher passes a fresh one, else fael makes it; a reused R is rejected
    pub run: Option<String>,
    /// → `run.client`; None = the approval's client, else the chunk's `client_hint`
    pub client: Option<String>,
    /// what the launcher runs it with → `run.model`; None = the approval's, else `model_hint`
    pub model: Option<String>,
    /// a client that writes a file and exits (image gen): no worktree, no slot; `done --run R`
    /// from the launching shell finishes it
    pub no_worktree: bool,
    /// take over a live run in another worktree (its end names who took it), never a kill;
    /// a live run in the caller's own worktree is taken over without it
    pub force: bool,
    /// the agent's session (`CLAUDE_CODE_SESSION_ID`) on a start without `--run`, for `--resume`
    pub session: Option<String>,
    /// HEAD's sha in the worktree at start → `run.base`, where `done`'s diff begins
    pub base: Option<String>,
    /// `.fael/shared` paths: nobody's claim, never an overlap (as on the board)
    pub shared: Vec<String>,
    pub stale: String, // a live run last seen before this is `stalled` in a clash line
    /// `--with`: the chunk whose live run this start joins as `joint` (issue 01M5PB7Z)
    pub with: Option<String>,
    pub joint: Option<Joint>,
}

/// One claimed chunk, with what its brief needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brief {
    pub uid: String,
    pub label: Option<String>,
    pub title: String,
    pub brief: String,
    pub size: Option<String>,
    /// the model this start runs: the run row's, else the approval's, else the hint
    pub model: Option<String>,
    pub scope: Option<String>,
    /// `app/name` or `name`
    pub plan: String,
    pub plan_title: String,
    /// the plan's sections in order
    pub sections: Vec<Section>,
    /// `(ref, section)` per `--reads` line, `None` = no such section
    pub reads: Vec<(String, Option<Section>)>,
    /// the owner's answers since this chunk last started (SPEC §1 "owner said")
    pub said: Vec<String>,
    /// `fael chunk note` texts since this chunk last started
    pub notes: Vec<String>,
    /// `(title, handoff, touched)`: the done / review word of each chunk this one is after,
    /// and the files its done claimed (one path per line, empty when none)
    pub after: Vec<(String, String, String)>,
    /// the same for done chunks that touched files under this scope since the brief was written
    /// (newest first, capped), and how many more there were
    pub by_file: Vec<(String, String, String)>,
    pub by_file_more: usize,
    /// live touches of running chunks under this scope (`chunk touch`, newest run first,
    /// capped): `(title, touched)` — no hand-off text yet, printed distinct from `by_file`
    pub live: Vec<(String, String)>,
    pub live_more: usize,
    /// the run this start took over or restarted — where the last agent stopped
    pub resume: Option<Prev>,
    /// the ledger issue ids it fixes (`add --issue`); the CLI swaps each for its row's text
    pub issues: Vec<String>,
}

/// The last run of a chunk a start takes over: what a resume needs to find its work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prev {
    pub start: String,
    pub client: Option<String>,
    pub session: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub started: String,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub run: String,
    pub chunks: Vec<Brief>,
    /// `(uid, title)` of other chunks still `running` whose live run this start ended in the
    /// worktree — informed, never blocked, their state untouched.
    pub orphaned: Vec<(String, String)>,
    /// Same-session retry: `reuse_live` returned the live R without a new run row.
    /// The caller prints short unless `--full`.
    pub resumed: bool,
}

fn prev(tx: &Transaction, id: i64) -> Result<Option<Prev>, String> {
    tx.query_row(
        "SELECT start, client, session, worktree, branch, started, last_seen
         FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1",
        [id],
        |r| {
            Ok(Prev {
                start: r.get(0)?,
                client: r.get(1)?,
                session: r.get(2)?,
                worktree: r.get(3)?,
                branch: r.get(4)?,
                started: r.get(5)?,
                last_seen: r.get(6)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

/// A reused R is rejected: one R per start.
fn check_fresh(tx: &Transaction, run: &str) -> Result<(), String> {
    let reused: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM run WHERE start = ?1)",
            [run],
            |r| r.get(0),
        )
        .map_err(err)?;
    if reused {
        return Err(format!(
            "rejected: run {run} was already used — one R per start — try `fael chunk start <uid> --run <a new R>`"
        ));
    }
    Ok(())
}

/// A retry from the session holding the chunk: the live brief again, not a
/// takeover. `None` = another session holds it (or none does) — the caller
/// runs the usual held/blocked checks.
fn reuse_live(
    tx: &Transaction,
    first: &super::chunk::Row,
    ms: &[String],
    session: Option<&str>,
) -> Result<Option<Started>, String> {
    let Some(session) = session else {
        return Ok(None);
    };
    if first.state != State::Running {
        return Ok(None);
    }
    let Some((live, held)) = holder_detail(tx, first.id)? else {
        return Ok(None);
    };
    if held.as_deref() != Some(session) {
        return Ok(None);
    }
    let same = ms.iter().all(|m| {
        row(tx, m).is_ok_and(|r| {
            r.state != State::Running
                || tx
                    .query_row(
                        "SELECT start FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
                        [r.id],
                        |q| q.get::<_, String>(0),
                    )
                    .is_ok_and(|s| s == live)
        })
    });
    if !same {
        return Ok(None);
    }
    let mut chunks = Vec::new();
    for m in ms {
        let r = row(tx, m)?;
        let mut b = brief(tx, r.id, 1, Some(&live))?;
        b.resume = prev(tx, r.id)?;
        chunks.push(b);
    }
    Ok(Some(Started {
        run: live,
        chunks,
        orphaned: Vec::new(),
        resumed: true,
    }))
}

/// The event text of a start past an unmet after (`--force` only); `None` = it was ready.
fn skipped(
    tx: &Transaction,
    id: i64,
    m: &str,
    pair: &str,
    ms: &[String],
    today: &str,
    force: bool,
) -> Result<Option<String>, String> {
    Ok(match ready_paired(tx, id, today, ms)? {
        Ready::Yes => None,
        Ready::Blocked(u) => {
            let w: Vec<_> = u.into_iter().map(|u| u.uid.unwrap_or(u.what)).collect();
            if !force {
                return Err(format!(
                    "rejected: chunk {m}{pair} is blocked — after {} first, or try `fael chunk start {m} --force` to start it anyway",
                    w.join(", ")
                ));
            }
            Some(format!("started by --force past after {}", w.join(", ")))
        }
        Ready::No(why) => {
            return Err(format!(
                "rejected: chunk {m}{pair} is not ready: {why} — try `fael chunk show {m}`"
            ));
        }
    })
}

/// One run row. `loc` = (worktree, branch, base); `with` = (joint mode, partner uid).
fn insert_run(
    tx: &Transaction,
    run: &str,
    chunk: i64,
    o: &Start,
    loc: (Option<&str>, Option<&str>, Option<&str>),
    with: Option<(&str, &str)>,
    now: &str,
) -> Result<(), String> {
    // issue 01M4XN3S: no --client/--model = the approval's, else the chunk's hint
    tx.execute(
        "INSERT INTO run(start, chunk, client, model, worktree, branch, started, session, base, joint, partner)
         SELECT ?1, id, COALESCE(?3, approved_client, client_hint),
                COALESCE(?4, approved_model, model_hint), ?5, ?6, ?7, ?8, ?9, ?10, ?11
         FROM chunk WHERE id = ?2",
        params![
            run, chunk, o.client, o.model, loc.0, loc.1, now, o.session, loc.2,
            with.map(|w| w.0), with.map(|w| w.1)
        ],
    )
    .map_err(err)?;
    Ok(())
}

/// One start per worktree: a run of another chunk still live here is an agent
/// that left (c5 split: keeps `Store::start` at the ratchet).
fn end_seats(
    tx: &Transaction,
    is_pair: bool,
    caller_wt: Option<&str>,
    wt: Option<&str>,
    ms: &[String],
    now: &str,
) -> Result<Vec<(String, String)>, String> {
    let orphaned = end_here(tx, wt.filter(|_| !is_pair), ms, now)?;
    end_caller_seat(tx, is_pair, caller_wt, wt, now)?;
    Ok(orphaned)
}

impl Store {
    pub fn start(&mut self, uid: &str, o: &Start, here: &Here) -> Result<Started, String> {
        let mut run = o.run.clone().unwrap_or_else(fael_core::ulid);
        let today = &here.now[..here.now.len().min(10)];
        let mut wt = (!o.no_worktree).then_some(here.worktree);
        let tx = tx(self)?;
        let first = row(&tx, uid)?;
        let mut claim = Vec::new();
        let ms = members(&tx, first.id)?;
        // a retry from the session that already holds it is success, not a
        // takeover: the brief header names the live R, so re-running start in
        // the same session must print that brief, never demand `--force`.
        if let Some(reused) = reuse_live(&tx, &first, &ms, o.session.as_deref())? {
            tx.commit().map_err(err)?;
            return Ok(reused);
        }
        let partner = joint::gate(&tx, &first, &ms, o)?;
        // a pair is one R in the partner's worktree and branch; any other start is its own R
        let seat = partner.as_ref().filter(|p| p.mode == Joint::Pair);
        let caller_wt = wt;
        let (mut branch, mut base) = (here.branch.map(String::from), o.base.clone());
        if let Some(p) = seat {
            joint::no_rejoin(&tx, &first, p)?;
        }
        match seat {
            Some(p) => {
                (run, wt, branch, base) = (
                    p.run.clone(),
                    p.worktree.as_deref(),
                    p.branch.clone(),
                    p.base.clone(),
                )
            }
            None => check_fresh(&tx, &run)?,
        }
        let joined = partner.as_ref().map(joint::Partner::text);
        // the handshake rides on the first member's start event
        let said = |m: &str, t: Option<String>| match (&joined, m == first.uid) {
            (Some(j), true) => Some(t.map_or_else(|| j.clone(), |t| format!("{t}; {j}"))),
            _ => t,
        };
        for m in ms.iter().cloned() {
            let r = row(&tx, &m)?;
            let pair = if m == first.uid {
                String::new()
            } else {
                format!(" (paired with {uid})")
            };
            let mut was = None;
            if r.state == State::Running {
                was = prev(&tx, r.id)?;
                // a running chunk whose run ended (the agent exited) starts again as is
                // the same worktree is the same seat: the agent that held it is gone
                let seat = wt.is_some() && holder_worktree(&tx, r.id)?.as_deref() == wt;
                let text = match holder(&tx, r.id)? {
                    Some(h) if !o.force && !seat => {
                        return Err(format!(
                            "rejected: chunk {m}{pair} is held by run {h} — try `fael chunk start {m} --force` to take it over"
                        ));
                    }
                    Some(h) => {
                        end_live(&tx, r.id, here.now)?;
                        let who = o.client.as_deref().unwrap_or("agent");
                        format!(
                            "taken over from run {h} by run {run} ({who}) in {}",
                            wt.unwrap_or("no worktree")
                        )
                    }
                    None => format!("started again by run {run}: the last run ended"),
                };
                set(
                    &tx,
                    &r,
                    State::Running,
                    "agent",
                    said(&m, Some(text)).as_deref(),
                    here.now,
                )?;
            } else {
                // a chunk whose work shipped before its after was met still needs a run
                // to reach done: `--force` starts it, and the event says what it skipped
                let text = skipped(&tx, r.id, &m, &pair, &ms, today, o.force)?;
                set(
                    &tx,
                    &r,
                    State::Running,
                    "agent",
                    said(&m, text).as_deref(),
                    here.now,
                )?;
            }
            claim.push((r, was));
        }
        let orphaned = end_seats(&tx, seat.is_some(), caller_wt, wt, &ms, here.now)?;
        let mut chunks = Vec::new();
        for (r, was) in claim {
            let loc = (wt, wt.and(branch.as_deref()), wt.and(base.as_deref()));
            let with = partner.as_ref().map(|p| (p.mode.as_str(), p.uid.as_str()));
            insert_run(&tx, &run, r.id, o, loc, with, here.now)?;
            unwait(&tx, r.id)?;
            let mut b = brief(&tx, r.id, 1, Some(&run))?;
            b.resume = was;
            chunks.push(b);
        }
        tx.commit().map_err(err)?;
        Ok(Started {
            run,
            chunks,
            orphaned,
            resumed: false,
        })
    }
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
