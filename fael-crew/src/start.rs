//! `fael chunk start <uid> [--run R] [--client C] [--force]` (SPEC §1 run rules, §4 pair):
//! claim the chunk and every chunk paired with it in one `BEGIN IMMEDIATE` — all or none —
//! insert one run row each under the same R, and hand back what the brief prints.
//! One worktree is one seat: a live, non-stalled run there from another known session refuses the
//! start (`--force --because` takes it); a stalled run, the caller's own session's run, or one
//! whose session (or the caller's) is unknown is an agent that left, so a start ends it.

use super::State;
use super::agent::unwait;
use super::chunk::{Here, Row, end_live, row, set, tx};
use super::holder::{
    end_caller_seat, end_here, holder, holder_detail, holder_worktree, live_in_worktree, members,
    ready_paired, stalled_since,
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
    /// `--force --because "<what you will avoid>"` (issue 01M6CZ0K): the receipt a
    /// force writes — stored on the run and in the start event. Bare `--force`
    /// is rejected; `--because` without `--force` is rejected.
    pub because: Option<String>,
    /// the agent's session (`CLAUDE_CODE_SESSION_ID`) on a start without `--run`, for `--resume`
    pub session: Option<String>,
    /// HEAD's sha in the worktree at start → `run.base`, where `done`'s diff begins
    pub base: Option<String>,
    pub stale: String, // a live run last seen before this is `stalled`: it holds no worktree
    /// `--with`: the chunks whose live runs this start joins as `joint` (issue 01M5PB7Z,
    /// f3: repeated `--with` names several partners, one `--as` for all of them)
    pub with: Vec<String>,
    pub joint: Option<Joint>,
    /// `--as stack`: the CLI's `merge-base --is-ancestor` read, parallel to the expanded
    /// `--with` order — true when the caller's HEAD already contains that partner's live
    /// branch tip. The store enforces it; the CLI computes it (pure DB logic stays testable).
    pub stack_contains: Vec<bool>,
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
    /// avoid cards (l3, issue 01M6CZ0K): one per running chunk whose scope collides
    /// with mine — who/class/files/where/when/doing/do. Newest run first, capped.
    pub avoid: Vec<super::handoff::Avoid>,
    pub avoid_more: usize,
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
    /// The force receipt this start wrote, if it forced anything past
    /// (`started by --force past … — because …`): the CLI prints it after
    /// the brief (issue 01M6CZ0K). Empty = a start that forced nothing.
    pub forced: Vec<String>,
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
    stale: &str,
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
        let mut b = brief(tx, r.id, 1, Some(&live), stale)?;
        b.resume = prev(tx, r.id)?;
        chunks.push(b);
    }
    Ok(Some(Started {
        run: live,
        chunks,
        orphaned: Vec::new(),
        resumed: true,
        forced: Vec::new(),
    }))
}

/// The event text of a start past an unmet after (`--force --because` only);
/// `None` = it was ready. Bare `--force` is rejected; the receipt carries
/// the past after plus `--because` (issue 01M6CZ0K).
fn skipped(
    tx: &Transaction,
    id: i64,
    m: &str,
    pair: &str,
    ms: &[String],
    today: &str,
    o: &Start,
) -> Result<Option<String>, String> {
    Ok(match ready_paired(tx, id, today, ms)? {
        Ready::Yes => None,
        Ready::Blocked(u) => {
            let w: Vec<_> = u.into_iter().map(|u| u.uid.unwrap_or(u.what)).collect();
            if !o.force {
                return Err(format!(
                    "rejected: chunk {m}{pair} is blocked — after {} first, or try `fael chunk start {m} --force --because \"<what you will avoid>\"` to start it anyway",
                    w.join(", ")
                ));
            }
            let because = need_because(o, m)?;
            Some(format!(
                "started by --force past after {} — because {because}",
                w.join(", ")
            ))
        }
        Ready::No(why) => {
            return Err(format!(
                "rejected: chunk {m}{pair} is not ready: {why} — {}",
                way_out(&why, m)
            ));
        }
    })
}

/// Bare `--force` is rejected; returns the trimmed receipt on a forced start.
/// Call only when `o.force` already held (the caller rejected the unforced
/// path); a non-force call is a programming error, rejected as one.
fn need_because(o: &Start, m: &str) -> Result<String, String> {
    if !o.force {
        return Err(format!(
            "crew.db: need_because called without --force for {m}"
        ));
    }
    match o
        .because
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        Some(b) => Ok(b.to_string()),
        None => Err(format!(
            "rejected: --force needs --because \"<what you will avoid>\" — try `fael chunk start {m} --force --because \"<what you will avoid>\"`"
        )),
    }
}

/// `--because` without `--force` names no bypass: reject it, so a receipt
/// never rides a start that forced nothing past.
fn check_lonely_because(o: &Start, m: &str) -> Result<(), String> {
    if !o.force && o.because.as_deref().is_some_and(|b| !b.trim().is_empty()) {
        return Err(format!(
            "rejected: --because needs --force — try `fael chunk start {m}` alone, or `fael chunk start {m} --force --because \"<what you will avoid>\"`"
        ));
    }
    Ok(())
}

/// What follows a not-ready reason. A wait on the owner is the owner's to end, so the verb is
/// named for them, never for the agent to type; `why` is `ready::ready_in`'s text.
fn way_out(why: &str, m: &str) -> String {
    match why.starts_with("it waits on owner") {
        true => format!(
            "the owner's answer unblocks it (`fael chunk answer {m} \"<text>\"` or the board); the question: `fael chunk show {m}`"
        ),
        false => format!("try `fael chunk show {m}`"),
    }
}

/// One run row. `loc` = (worktree, branch, base); `with` = the joint partners, first in
/// `run.partner`, the rest as JSON in `run.partners` (f3, additive column).
/// `force_reason` = `--because` trimmed, only on a `--force` start, else NULL.
fn insert_run(
    tx: &Transaction,
    run: &str,
    chunk: i64,
    o: &Start,
    loc: (Option<&str>, Option<&str>, Option<&str>),
    with: &[super::joint::Partner],
    now: &str,
) -> Result<(), String> {
    // issue 01M4XN3S: no --client/--model = the approval's, else the chunk's hint
    let force_reason = o
        .force
        .then(|| o.because.as_deref().map(str::trim))
        .flatten()
        .filter(|b| !b.is_empty());
    let rest = (with.len() > 1).then(|| {
        serde_json::to_string(&with[1..].iter().map(|p| &p.uid).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".into())
    });
    tx.execute(
        "INSERT INTO run(start, chunk, client, model, worktree, branch, started, session, base, joint, partner, partners, force_reason)
         SELECT ?1, id, COALESCE(?3, approved_client, client_hint),
                COALESCE(?4, approved_model, model_hint), ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
         FROM chunk WHERE id = ?2",
        params![
            run,
            chunk,
            o.client,
            o.model,
            loc.0,
            loc.1,
            now,
            o.session,
            loc.2,
            with.first().map(|p| p.mode.as_str()),
            with.first().map(|p| p.uid.as_str()),
            rest,
            force_reason
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

/// The one refusal left of the old overlap gate (issue 01M6KHWF): another live, non-stalled run
/// in the worktree this start sits in (or leaves, for a pair), from a different known session —
/// two agents there overwrite each other's files. The caller's own session never refuses itself,
/// and an unknown session (either side) never refuses: a wrong refusal is the cost to avoid. A stalled run is no refusal: `end_seats` ends it as before. Overlap of
/// declared scopes never refuses. `--force --because` takes the seat; returns that receipt.
fn seat_taken(
    tx: &Transaction,
    uid: &str,
    seat: Option<&joint::Partner>,
    worktrees: &[Option<&str>],
    ms: &[String],
    o: &Start,
) -> Result<Option<String>, String> {
    let except = seat.map(|p| p.run.as_str());
    let mut told = Vec::new();
    for w in worktrees.iter().flatten() {
        for (other, run) in live_in_worktree(tx, w, ms, except, o.session.as_deref(), &o.stale)? {
            let t = format!("run {run} of chunk {other} in {w}");
            if !told.contains(&t) {
                told.push(t);
            }
        }
    }
    if told.is_empty() {
        return Ok(None);
    }
    let told = told.join("; ");
    if !o.force {
        return Err(format!(
            "rejected: chunk {uid} cannot start here — {told} is live, and two agents in one worktree overwrite each other's files; wait for it, start from another worktree, or try `fael chunk start {uid} --force --because \"<what you will avoid>\"` to end it"
        ));
    }
    let because = need_because(o, uid)?;
    Ok(Some(format!(
        "started by --force past live {told} — because {because}"
    )))
}

/// The event text for taking over a live holder: a stalled holder goes without
/// `--force`, its stall as the receipt (fael:01M6CZ9G41); a live one needs
/// `--force --because`, and the receipt rides `forced` (issue 01M6CZ0K).
#[allow(clippy::too_many_arguments)]
fn held_takeover(
    tx: &Transaction,
    r_id: i64,
    h: String,
    m: &str,
    pair: &str,
    o: &Start,
    run: &str,
    wt: Option<&str>,
    now: &str,
    seat: bool,
    forced: &mut Vec<String>,
) -> Result<String, String> {
    if !o.force && !seat {
        if let Some(since) = stalled_since(tx, r_id, &o.stale)? {
            end_live(tx, r_id, now)?;
            let who = o.client.as_deref().unwrap_or("agent");
            return Ok(format!(
                "taken over from run {h} by run {run} ({who}) in {} — partner {m} stalled since {since}",
                wt.unwrap_or("no worktree")
            ));
        }
        return Err(format!(
            "rejected: chunk {m}{pair} is held by run {h} — try `fael chunk start {m} --force --because \"<what you will avoid>\"` to take it over"
        ));
    }
    end_live(tx, r_id, now)?;
    let who = o.client.as_deref().unwrap_or("agent");
    if o.force {
        let because = need_because(o, m)?;
        let t = format!(
            "taken over from run {h} by run {run} ({who}) in {} by --force — because {because}",
            wt.unwrap_or("no worktree")
        );
        forced.push(t.clone());
        Ok(t)
    } else {
        Ok(format!(
            "taken over from run {h} by run {run} ({who}) in {}",
            wt.unwrap_or("no worktree")
        ))
    }
}

/// Claim every paired member: write its start event (joint handshake and force
/// receipts ride on the first member) and return the rows plus the force
/// receipts for `Started.forced` (issue 01M6CZ0K).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn claim_members(
    tx: &Transaction,
    ms: &[String],
    first: &Row,
    uid: &str,
    o: &Start,
    run: &str,
    wt: Option<&str>,
    here: &Here,
    today: &str,
    joined: &Option<String>,
    seat_forced: &Option<String>,
) -> Result<(Vec<(Row, Option<Prev>)>, Vec<String>), String> {
    // the handshake and the clash receipt ride on the first member's start event
    let said = |m: &str, t: Option<String>| {
        if m != first.uid {
            return t;
        }
        let mut parts = Vec::new();
        if let Some(t) = t {
            parts.push(t);
        }
        if let Some(j) = joined {
            parts.push(j.clone());
        }
        if let Some(f) = seat_forced {
            parts.push(f.clone());
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    };
    let mut forced: Vec<String> = seat_forced.clone().into_iter().collect();
    let mut claim = Vec::new();
    for m in ms.iter().cloned() {
        let r = row(tx, &m)?;
        let pair = if m == first.uid {
            String::new()
        } else {
            format!(" (paired with {uid})")
        };
        let mut was = None;
        if r.state == State::Running {
            was = prev(tx, r.id)?;
            // a running chunk whose run ended (the agent exited) starts again as is
            // the same worktree is the same seat: the agent that held it is gone
            let seat = wt.is_some() && holder_worktree(tx, r.id)?.as_deref() == wt;
            let text = match holder(tx, r.id)? {
                Some(h) => held_takeover(
                    tx,
                    r.id,
                    h,
                    &m,
                    &pair,
                    o,
                    run,
                    wt,
                    here.now,
                    seat,
                    &mut forced,
                )?,
                None => {
                    if o.force {
                        let because = need_because(o, &m)?;
                        let t = format!(
                            "started again by run {run} by --force: the last run ended — because {because}"
                        );
                        forced.push(t.clone());
                        t
                    } else {
                        format!("started again by run {run}: the last run ended")
                    }
                }
            };
            set(
                tx,
                &r,
                State::Running,
                "agent",
                said(&m, Some(text)).as_deref(),
                here.now,
            )?;
        } else {
            // a chunk whose work shipped before its after was met still needs a run
            // to reach done: `--force --because` starts it, and the event says what it skipped
            let mut text = skipped(tx, r.id, &m, &pair, ms, today, o)?;
            if let Some(t) = &text {
                if t.contains("--force") {
                    forced.push(t.clone());
                }
            } else if o.force && seat_forced.is_none() {
                let because = need_because(o, &m)?;
                let t = format!("started by --force — because {because}");
                forced.push(t.clone());
                text = Some(t);
            }
            set(
                tx,
                &r,
                State::Running,
                "agent",
                said(&m, text).as_deref(),
                here.now,
            )?;
        }
        claim.push((r, was));
    }
    Ok((claim, forced))
}

impl Store {
    pub fn start(&mut self, uid: &str, o: &Start, here: &Here) -> Result<Started, String> {
        let mut run = o.run.clone().unwrap_or_else(fael_core::ulid);
        let today = &here.now[..here.now.len().min(10)];
        let mut wt = (!o.no_worktree).then_some(here.worktree);
        let tx = tx(self)?;
        let first = row(&tx, uid)?;
        let ms = members(&tx, first.id)?;
        // a retry from the session that already holds it is success, not a
        // takeover: the brief header names the live R, so re-running start in
        // the same session must print that brief, never demand `--force`.
        if let Some(reused) = reuse_live(&tx, &first, &ms, o.session.as_deref(), &o.stale)? {
            tx.commit().map_err(err)?;
            return Ok(reused);
        }
        check_lonely_because(o, uid)?;
        let partners = joint::gate(&tx, &first, &ms, o, here)?;
        // a pair is one R in the partner's worktree and branch; a stack keeps its own R,
        // worktree and branch cut from the partner's branch — any other start is its own R
        let seat = partners.iter().find(|p| p.mode == Joint::Pair);
        let caller_wt = wt;
        let (mut branch, mut base) = (here.branch.map(String::from), o.base.clone());
        if seat.is_some() {
            joint::no_rejoin(&tx, &first, &partners)?;
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
        let joined = partners
            .iter()
            .map(joint::Partner::text)
            .reduce(|a, b| format!("{a}; {b}"));
        let seat_forced = seat_taken(&tx, uid, seat, &[caller_wt, wt], &ms, o)?;
        let (claim, forced) = claim_members(
            &tx,
            &ms,
            &first,
            uid,
            o,
            &run,
            wt,
            here,
            today,
            &joined,
            &seat_forced,
        )?;
        let orphaned = end_seats(&tx, seat.is_some(), caller_wt, wt, &ms, here.now)?;
        let mut chunks = Vec::new();
        for (r, was) in claim {
            let loc = (wt, wt.and(branch.as_deref()), wt.and(base.as_deref()));
            insert_run(&tx, &run, r.id, o, loc, &partners, here.now)?;
            unwait(&tx, r.id)?;
            let mut b = brief(&tx, r.id, 1, Some(&run), &o.stale)?;
            b.resume = was;
            chunks.push(b);
        }
        tx.commit().map_err(err)?;
        Ok(Started {
            run,
            chunks,
            orphaned,
            resumed: false,
            forced,
        })
    }
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
