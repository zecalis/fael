//! Joint start (plan overlap-broker o2, issue 01M5PB7Z): `chunk start <uid> --with <partner>
//! --as pair|lane|follow|stack` records a handshake on the run row and the start event — crew.db
//! only, no daemon. Judges nothing: the caller names the mode. Optional: an overlap never needs it.

use super::chunk::{Here, Row};
use super::holder::holder_detail;
use super::start::Start;
use super::store::err;
use rusqlite::{OptionalExtension, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    /// one R, the partner's worktree and branch
    Pair,
    /// own worktree, the owner merges in any order
    Lane,
    /// read-only: its handoff reaches the brief by file
    Follow,
    /// own R, worktree and branch, cut from the partner's branch; the caller's HEAD must
    /// already contain the partner's live branch tip
    Stack,
}

impl Joint {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "pair" => Ok(Self::Pair),
            "lane" => Ok(Self::Lane),
            "follow" => Ok(Self::Follow),
            "stack" => Ok(Self::Stack),
            _ => Err(format!(
                "rejected: --as {s} is not pair, lane, follow or stack; try --as pair"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pair => "pair",
            Self::Lane => "lane",
            Self::Follow => "follow",
            Self::Stack => "stack",
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

/// The start's joint gate: names the partners (`--with`/`--as`) and, for a `stack`, checks the
/// caller's HEAD contains each partner's live branch tip (`o.stack_contains`, the CLI's
/// `merge-base --is-ancestor` read), else rejects with the ready `git switch -c` line.
/// Declared-scope overlap with another live run is no refusal here (issue 01M6KHWF): the
/// brief's avoid card says it, `fael chunk status` finds a real conflict.
pub(super) fn gate(
    tx: &Transaction,
    first: &Row,
    ms: &[String],
    o: &Start,
    here: &Here,
) -> Result<Vec<Partner>, String> {
    let ps = partners(tx, first, ms, o)?;
    if o.joint == Some(Joint::Stack) {
        if o.stack_contains.len() != ps.len() {
            return Err(format!(
                "crew.db: stack needs ancestor for every partner ({} with, {} checked)",
                ps.len(),
                o.stack_contains.len()
            ));
        }
        for (p, ok) in ps.iter().zip(o.stack_contains.iter()) {
            if !ok {
                let pb = p.branch.as_deref().unwrap_or("<partner-branch>");
                let cb = here.branch.unwrap_or("<branch>");
                return Err(format!(
                    "rejected: chunk {} stacks on {} but HEAD does not contain {pb} tip — try `git switch -c {cb} {pb}` then start again",
                    first.uid, p.uid
                ));
            }
        }
    }
    Ok(ps)
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

    /// `--as stack`: the live run's branch of the chunk `key` names (uid, label or prefix),
    /// if it has one. The CLI resolves its tip and checks the caller's HEAD contains it;
    /// the store enforces the result (`Start.stack_contains`). None = no live run or no
    /// branch recorded — the store's `join` says which.
    pub fn live_branch(&self, key: &str) -> Result<Option<String>, String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        let Some((id, _, _)) = super::lookup::lookup(&tx, key)? else {
            return Ok(None);
        };
        tx.query_row(
            "SELECT branch FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .map_err(err)
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
