//! The owner's half (SPEC §1, §5): commands from the board, never fenced, and `run end`.

use super::chunk::{end_live, row, set, tx};
use super::store::{Store, err};
use super::{State, allowed, check};
use rusqlite::params;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Answer,
    Accept,
    Drop,
    Park,
    Unpark,
}

impl Owner {
    pub const ALL: [Owner; 5] = [
        Owner::Answer,
        Owner::Accept,
        Owner::Park,
        Owner::Unpark,
        Owner::Drop,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Owner::Answer => "answer",
            Owner::Accept => "accept",
            Owner::Drop => "drop",
            Owner::Park => "park",
            Owner::Unpark => "unpark",
        }
    }

    /// Does this command take a chunk in `s`? The one rule `owner()` rejects by and the
    /// board's `owner_actions` lists, so the app keeps no copy of it.
    pub fn takes(self, s: State) -> bool {
        match self {
            Owner::Answer => matches!(s, State::Waiting | State::Review),
            Owner::Accept => s == State::Review,
            Owner::Unpark => s == State::Parked,
            Owner::Park => allowed(s, State::Parked),
            Owner::Drop => allowed(s, State::Dropped),
        }
    }

    /// The owner verbs a chunk in `state` takes now, in `ALL` order (board `owner_actions`).
    pub fn actions(state: &str) -> Vec<&'static str> {
        State::parse(state).map_or_else(Vec::new, |s| {
            Owner::ALL
                .into_iter()
                .filter(|o| o.takes(s))
                .map(Owner::as_str)
                .collect()
        })
    }
}

impl Store {
    /// Owner commands from the board, never fenced: `answer` (waiting or review → open, the
    /// next start's brief prints it), `accept` (review → done), `drop`, `park`, `unpark`.
    pub fn owner(
        &mut self,
        uid: &str,
        cmd: Owner,
        text: Option<&str>,
        now: &str,
    ) -> Result<(), String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        let to = match cmd {
            Owner::Answer | Owner::Unpark => State::Open,
            Owner::Accept => State::Done,
            Owner::Drop => State::Dropped,
            Owner::Park => State::Parked,
        };
        if !cmd.takes(r.state) {
            let st = r.state.as_str();
            return Err(match cmd {
                Owner::Answer => format!(
                    "rejected: chunk {uid} is {st} — `answer` replies to a waiting chunk or sends a review back"
                ),
                Owner::Accept => {
                    format!("rejected: chunk {uid} is {st} — `accept` takes a chunk in review")
                }
                Owner::Unpark => format!("rejected: chunk {uid} is {st}, not parked"),
                // the state table's reject; set() skips it when the state stays (park a parked)
                Owner::Park | Owner::Drop => {
                    format!("{} ({uid})", check(r.state, to).err().unwrap_or_default())
                }
            });
        }
        set(&tx, &r, to, "owner", text, now)?;
        if to == State::Open {
            tx.execute(
                "UPDATE chunk SET wait_on = NULL, wait_text = NULL, wait_until = NULL WHERE id = ?1",
                [r.id],
            )
            .map_err(err)?;
        }
        end_live(&tx, r.id, now)?;
        tx.commit().map_err(err)
    }

    /// `fael chunk pin <uid> <rank>` / `unpin` (None): the owner's rank, first in queue
    /// order (SPEC §3), lowest first. State and approval stay.
    pub fn pin(&mut self, uid: &str, rank: Option<i64>, now: &str) -> Result<(), String> {
        let text = rank.map_or("unpinned".into(), |n| format!("pinned {n}"));
        self.mark(uid, &text, now, |tx, id| {
            tx.execute("UPDATE chunk SET pin = ?1 WHERE id = ?2", params![rank, id])
        })
    }

    /// `fael chunk approve <uid> --model M --client C` / `unapprove` (None): the approved
    /// queue may start it with that model and client when a slot frees (SPEC §8); `edit`
    /// clears it.
    pub fn approve(
        &mut self,
        uid: &str,
        with: Option<(&str, &str)>,
        now: &str,
    ) -> Result<(), String> {
        let text = with.map_or("unapproved".into(), |(m, c)| {
            format!("approved · model {m} · client {c}")
        });
        let (m, c) = with.unzip();
        self.mark(uid, &text, now, |tx, id| {
            tx.execute(
                "UPDATE chunk SET approved_at = ?1, approved_model = ?2, approved_client = ?3
                 WHERE id = ?4",
                params![m.map(|_| now), m, c, id],
            )
        })
    }

    /// An owner field on a chunk still to run, with its event; the state stays.
    fn mark(
        &mut self,
        uid: &str,
        text: &str,
        now: &str,
        f: impl FnOnce(&rusqlite::Transaction, i64) -> rusqlite::Result<usize>,
    ) -> Result<(), String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        if r.state.terminal() {
            return Err(format!(
                "rejected: chunk {uid} is {} — history, never queued",
                r.state.as_str()
            ));
        }
        f(&tx, r.id).map_err(err)?;
        set(&tx, &r, r.state, "owner", Some(text), now)?;
        tx.commit().map_err(err)
    }

    /// `fael run end R [--cost USD]`: ends every live row of start R and nothing else —
    /// never a chunk's state, never another start. None left (wait/done ended them) is not
    /// an error. `cost` lands on every row of R, ended or not: the agent's own `chunk done`
    /// usually ended the run before the launcher reports what it cost.
    pub fn run_end(&mut self, run: &str, cost: Option<f64>, now: &str) -> Result<usize, String> {
        let tx = self.conn.transaction().map_err(err)?;
        let n = tx
            .execute(
                "UPDATE run SET ended = ?1 WHERE start = ?2 AND ended IS NULL",
                params![now, run],
            )
            .map_err(err)?;
        if cost.is_some() {
            tx.execute(
                "UPDATE run SET cost = ?1 WHERE start = ?2",
                params![cost, run],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(n)
    }
}
