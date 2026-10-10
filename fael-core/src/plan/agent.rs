//! The agent's half of the contract (SPEC §1): `wait`, `after`, `done` — fenced to the
//! worktree of the chunk's latest run, each ending its live run in the same transaction —
//! and the Stop hook's stamp on the live run of its own worktree.

use super::State;
use super::chunk::{Here, add_after, end_live, fence, is_date, row, set, tx, uid_id};
use super::store::{Store, err};
use rusqlite::params;

/// `done --out`: copy the output for run R, return where it went.
pub type Copy<'a> = dyn FnMut(&str) -> Result<String, String> + 'a;

impl Store {
    /// `fael chunk wait --on owner|data "<question>" [--until D]`: the run ends, the question stays.
    pub fn wait(
        &mut self,
        uid: &str,
        on: &str,
        text: &str,
        until: Option<&str>,
        here: &Here,
    ) -> Result<(), String> {
        if !matches!(on, "owner" | "data") {
            return Err(format!("rejected: --on {on} — owner or data"));
        }
        if until.is_some_and(|u| !is_date(u)) {
            return Err("rejected: --until takes a date YYYY-MM-DD".into());
        }
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        fence(&tx, &r, here)?;
        set(&tx, &r, State::Waiting, "agent", Some(text), here.now)?;
        tx.execute(
            "UPDATE chunk SET wait_on = ?1, wait_text = ?2, wait_until = ?3 WHERE id = ?4",
            params![on, text, until, r.id],
        )
        .map_err(err)?;
        end_live(&tx, r.id, here.now)?;
        tx.commit().map_err(err)
    }

    /// `fael chunk after <uid> <other> "<why>"`: needs `other` first. A running chunk goes
    /// back to `open` and its run ends; `blocked` follows from the edge (SPEC §2).
    pub fn after(&mut self, uid: &str, other: &str, why: &str, here: &Here) -> Result<(), String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        fence(&tx, &r, here)?;
        let dst = uid_id(&tx, other)?;
        if dst == r.id {
            return Err("rejected: a chunk cannot wait on itself".into());
        }
        add_after(&tx, r.id, dst)?;
        let to = if r.state == State::Running {
            end_live(&tx, r.id, here.now)?;
            State::Open
        } else if r.state.terminal() || r.state == State::Review {
            return Err(format!(
                "rejected: chunk {uid} is {} — an `after` only holds back work not yet done",
                r.state.as_str()
            ));
        } else {
            r.state
        };
        set(
            &tx,
            &r,
            to,
            "agent",
            Some(&format!("after {other}: {why}")),
            here.now,
        )?;
        tx.commit().map_err(err)
    }

    /// `fael chunk done "<handoff>" [--pr N | --out <path>]`: with a PR → done (the owner's
    /// push pr is the ok; a later problem is a new chunk or issue), else review. `out` copies the
    /// output for this run R (inside the transaction: a failed copy changes nothing) and
    /// returns the path kept in `run.out`.
    pub fn done(
        &mut self,
        uid: &str,
        handoff: &str,
        pr: Option<i64>,
        out: Option<&mut Copy<'_>>,
        here: &Here,
    ) -> Result<(), String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        fence(&tx, &r, here)?;
        let to = if pr.is_some() {
            State::Done
        } else {
            State::Review
        };
        set(&tx, &r, to, "agent", Some(handoff), here.now)?;
        let (run, start): (i64, String) = tx
            .query_row(
                "SELECT id, start FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1",
                [r.id],
                |q| Ok((q.get(0)?, q.get(1)?)),
            )
            .map_err(err)?;
        let out = out.map(|f| f(&start)).transpose()?;
        tx.execute(
            "UPDATE run SET pr = COALESCE(?1, pr), out = COALESCE(?2, out) WHERE id = ?3",
            params![pr, out, run],
        )
        .map_err(err)?;
        end_live(&tx, r.id, here.now)?;
        tx.commit().map_err(err)
    }

    /// Live runs in a worktree, `(uid, title, R)` — what a resumed session holds.
    pub fn held(&self, worktree: &str) -> Result<Vec<(String, String, String)>, String> {
        self.conn
            .prepare(
                "SELECT c.uid, c.title, r.start FROM run r JOIN chunk c ON c.id = r.chunk
                 WHERE r.worktree = ?1 AND r.ended IS NULL ORDER BY c.seq",
            )
            .map_err(err)?
            .query_map([worktree], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)
    }

    /// The Stop hook (SPEC §7): the live run of its own worktree gets `last_seen`, and
    /// `session` when empty. A run naming another session is left alone; an ended run is
    /// never written.
    pub fn seen(&mut self, worktree: &str, session: &str, now: &str) -> Result<usize, String> {
        self.conn
            .execute(
                "UPDATE run SET session = COALESCE(session, ?2), last_seen = ?3
                 WHERE worktree = ?1 AND ended IS NULL AND (session IS NULL OR session = ?2)",
                params![worktree, session, now],
            )
            .map_err(err)
    }
}
