//! Joint start (plan overlap-broker o2, issue 01M5PB7Z): `chunk start <uid> --with <partner>
//! --as pair|lane|follow` turns a clash into a handshake recorded on the run row and the start
//! event — crew.db only, no daemon. Judges nothing: the caller names the mode; `--force` stays.

use super::chunk::Row;
use super::holder::{clash_report, holder_detail};
use super::start::Start;
use super::store::err;
use fael_core::OverlapClass;
use rusqlite::{OptionalExtension, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    /// one R, the partner's worktree and branch
    Pair,
    /// own worktree; a dir-level overlap is no refusal, the owner merges in any order
    Lane,
    /// read-only: no clash against the partner, its handoff reaches the brief by file
    Follow,
}

impl Joint {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "pair" => Ok(Self::Pair),
            "lane" => Ok(Self::Lane),
            "follow" => Ok(Self::Follow),
            _ => Err(format!(
                "rejected: --as {s} is not pair, lane or follow; try --as pair"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pair => "pair",
            Self::Lane => "lane",
            Self::Follow => "follow",
        }
    }
}

/// The partner's live run: what a pair start joins.
pub(super) struct Partner {
    pub uid: String,
    pub mode: Joint,
    pub run: String,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
}

impl Partner {
    /// the event / brief line that records the handshake
    pub fn text(&self) -> String {
        format!(
            "joint {} with {} (run {})",
            self.mode.as_str(),
            self.uid,
            self.run
        )
    }
}

/// `--with`/`--as` → the partners' live runs. Empty = a solo start.
fn partners(
    tx: &Transaction,
    first: &Row,
    ms: &[String],
    o: &Start,
) -> Result<Vec<Partner>, String> {
    // repeated `--with a --with b` names several; one `--with a,b` splits the same way
    let with: Vec<String> = o
        .with
        .iter()
        .flat_map(|w| w.split(','))
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    match (with.is_empty(), o.joint) {
        (true, None) => return Ok(Vec::new()),
        (true, Some(_)) | (false, None) => {
            return Err(
                "rejected: --with and --as go together; try `fael chunk start <uid> --with <partner> --as pair`".into(),
            );
        }
        (false, Some(_)) => {}
    }
    with.iter()
        .map(|w| {
            join(
                tx,
                first,
                ms,
                w,
                o.joint.expect("joint checked with --with"),
            )
        })
        .collect()
}

/// The live run of the chunk `with` names, joined as `mode`.
fn join(
    tx: &Transaction,
    first: &Row,
    ms: &[String],
    with: &str,
    mode: Joint,
) -> Result<Partner, String> {
    let p = super::lookup::lookup(tx, with)?.ok_or_else(|| super::lookup::no_chunk(with))?;
    let uid = p.1.clone();
    if p.0 == first.id || ms.contains(&uid) {
        return Err(format!(
            "rejected: --with {with} is this chunk (or its pair) — name another; try --with <another-chunk> --as pair"
        ));
    }
    let Some((run, _)) = holder_detail(tx, p.0)? else {
        return Err(format!(
            "rejected: --with {with} has no live run — try `fael chunk start {with}` first, or start this one alone"
        ));
    };
    let (worktree, branch, base) = tx
        .query_row(
            "SELECT worktree, branch, base FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
            [p.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(err)?
        .unwrap_or_default();
    Ok(Partner {
        uid,
        mode,
        run,
        worktree,
        branch,
        base,
    })
}

/// `(partners, force receipt, auto-lane note)`.
pub(super) type Gate = (Vec<Partner>, Option<String>, Option<String>);

/// How a stalled clash line ends (`holder::clash_report`).
const STALLED: &str = " · stalled";

/// Every chunk uid in the partners' live runs: a joint start works beside them all (f3).
fn run_members(tx: &Transaction, ps: &[Partner]) -> Result<Vec<String>, String> {
    if ps.is_empty() {
        return Ok(Vec::new());
    }
    let marks = ps.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT c.uid FROM run r JOIN chunk c ON c.id = r.chunk
         WHERE r.ended IS NULL AND r.start IN ({marks})"
    );
    let runs: Vec<&String> = ps.iter().map(|p| &p.run).collect();
    tx.prepare(&sql)
        .and_then(|mut q| {
            q.query_map(rusqlite::params_from_iter(runs), |r| r.get(0))?
                .collect()
        })
        .map_err(err)
}

/// The refusal naming every remaining clash with its ready `--with` command (f3: one
/// `--with <uid>` per clash, one `--as` for all). `exact_lane` marks a lane-over-exact
/// hint that must be a pair instead.
fn refuse(
    uid: &str,
    kept: &[&(String, OverlapClass, String)],
    how: &str,
    exact_lane: impl Fn(&str, OverlapClass) -> bool,
    force: &str,
    o: &Start,
    joined_lines: &str,
) -> Result<Gate, String> {
    let flags = kept
        .iter()
        .map(|k| format!("--with {} --as {how}", k.0))
        .collect::<Vec<_>>()
        .join(" ");
    let way = format!(
        "try `fael chunk start {uid} {flags}` to start beside {them}, or {force} to start it anyway",
        them = if kept.len() == 1 { "it" } else { "them" }
    );
    let why = if kept.iter().any(|k| exact_lane(&k.0, k.1)) {
        " (--as lane over the same file is a pair)"
    } else {
        ""
    };
    let because = o
        .because
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .filter(|_| o.force);
    let Some(because) = because else {
        let bare = if o.force {
            " (bare --force is rejected)"
        } else {
            ""
        };
        return Err(format!(
            "rejected: chunk {uid} shares files with a running chunk — {joined_lines}; {way}{why}{bare}"
        ));
    };
    Ok((
        Vec::new(),
        Some(format!(
            "started by --force past clash {joined_lines} — because {because}"
        )),
        None,
    ))
}

/// The start's clash gate. A clash with the partner is no refusal for `follow`, `pair`, or a
/// `lane` over a dir (prefix) overlap; a lane over the same file still refuses — that is a
/// pair. Any other clash (or none named) refuses with the ready joint command, `--force
/// --because` last. Bare `--force` is rejected (issue 01M6CZ0K); a force past a clash
/// writes a receipt (clash snapshot + because, gap (1) in fael:01M6D140) the caller
/// stores in the start event.
/// `seated` = the chunk already runs in the caller's worktree: it holds its files, so a chunk
/// that started beside it later is no reason to refuse a restart (`start --full` to re-read).
/// A dir (prefix) overlap or a stalled holder is no refusal: the start lanes beside it with a
/// receipt, no `--with` needed, however many there are (fael:01M6CZ9G41). Only a live
/// same-file clash refuses; repeated `--with` (f3) names one partner per clash.
/// Returns the partners, on a forced clash its receipt, and the auto-lane note.
pub(super) fn gate(
    tx: &Transaction,
    first: &Row,
    ms: &[String],
    o: &Start,
    seated: bool,
) -> Result<Gate, String> {
    let ps = partners(tx, first, ms, o)?;
    let report = clash_report(tx, ms, &o.shared, &o.stale)?;
    // chunks under any partner's run work beside it: no clash for a pair, follow or dir lane
    let in_run = run_members(tx, &ps)?;
    let mode = o.joint;
    let joined = |u: &str| ps.iter().any(|p| p.uid == u) || in_run.iter().any(|r| r == u);
    let exact_lane = |u: &str, c: OverlapClass| {
        mode == Some(Joint::Lane) && c == OverlapClass::Exact && joined(u)
    };
    let mut kept: Vec<_> = report
        .iter()
        .filter(|(u, c, _)| !(joined(u) && !exact_lane(u, *c)))
        .collect();
    let mut eased = Vec::new();
    if seated {
        kept.clear();
    } else {
        kept.retain(|k| {
            let easy = k.1 != OverlapClass::Exact || k.2.ends_with(STALLED);
            if easy {
                eased.push((k.0.as_str(), k.2.as_str()));
            }
            !easy
        });
    }
    let lines: Vec<_> = eased.iter().map(|e| e.1).collect();
    let lane = (!eased.is_empty()).then(|| format!("lane beside {}", lines.join("; ")));
    // the run row keeps one partner: the first, so its files are no "outside scope" for this lane
    let auto = match (ps.is_empty(), eased.first()) {
        (true, Some((u, _))) if !o.force => vec![join(tx, first, ms, u, Joint::Lane)?],
        _ => Vec::new(),
    };
    let uid = &first.uid;
    let because = o
        .because
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty());
    if kept.is_empty() {
        if o.force && because.is_none() {
            return Err(format!(
                "rejected: --force needs --because \"<what you will avoid>\" — try `fael chunk start {uid} --force --because \"<what you will avoid>\"`"
            ));
        }
        let mut all = ps;
        all.extend(auto);
        return Ok((all, None, lane));
    }
    let (hit, class, _) = kept[0];
    let how = if kept.iter().any(|(_, c, _)| *c == OverlapClass::Exact) || exact_lane(hit, *class) {
        "pair"
    } else {
        "lane"
    };
    let lines: Vec<_> = kept.iter().map(|k| k.2.as_str()).collect();
    let joined_lines = lines.join("; ");
    let force = format!("`fael chunk start {uid} --force --because \"<what you will avoid>\"`");
    let refused = refuse(uid, &kept, how, exact_lane, &force, o, &joined_lines)?;
    Ok((ps, refused.1, lane))
}

impl super::store::Store {
    /// `--as follow` output: the read-only contract (advice — fael blocks no edit) and the
    /// partner's handoff so far: its checkpoint notes since its last start, else its last
    /// done / review word. Empty handoff is said, never guessed.
    pub fn follow_text(&self, partner: &str) -> Result<String, String> {
        let notes: Vec<String> = self
            .conn
            .prepare(
                "SELECT text FROM event WHERE chunk_uid = ?1 AND by = 'note' AND text IS NOT NULL
                   AND id > COALESCE((SELECT MAX(id) FROM event WHERE chunk_uid = ?1
                                        AND to_state = 'running' AND by != 'note'), 0)
                 UNION ALL
                 SELECT text FROM event WHERE id = (SELECT MAX(id) FROM event WHERE chunk_uid = ?1
                   AND by = 'agent' AND to_state IN ('done', 'review')) AND text != ''
                 ORDER BY 1",
            )
            .and_then(|mut q| q.query_map([partner], |r| r.get(0))?.collect())
            .map_err(err)?;
        let mut o = format!(
            "follow: read-only — read {partner}'s work, edit nothing in its scope (advice; fael blocks no edit)\n"
        );
        if notes.is_empty() {
            o += &format!("{partner} has handed off nothing yet\n");
        } else {
            o += &format!("{partner} handed off:\n");
            for n in notes {
                o += &format!("> {n}\n");
            }
        }
        Ok(o)
    }
}

/// One run row per (R, chunk): a chunk that already ran under a partner's R cannot join it
/// again — it starts alone, or in a lane.
pub(super) fn no_rejoin(tx: &Transaction, first: &Row, ps: &[Partner]) -> Result<(), String> {
    for p in ps {
        let had: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM run WHERE start = ?1 AND chunk = ?2)",
                rusqlite::params![p.run, first.id],
                |r| r.get(0),
            )
            .map_err(err)?;
        if had {
            return Err(format!(
                "rejected: chunk {} already ran under run {} — try `fael chunk start {} --with {} --as lane`",
                first.uid, p.run, first.uid, p.uid
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "joint_tests.rs"]
mod tests;
