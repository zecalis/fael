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
            super::agent::unwait(&tx, r.id)?;
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

    /// `fael plan new <name> "<title>"`: a plan whose chunks live in the db from the start
    /// (`truth = db`, active) — no md chunk list to cut over. Its text starts as the four
    /// skeleton sections every brief reads (the plan has no md file). A taken `app/name` is refused.
    pub fn new_plan(
        &mut self,
        app: &str,
        name: &str,
        title: &str,
        area: &str,
        now: &str,
    ) -> Result<(), String> {
        let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if name.is_empty() || !name.chars().all(ok) || name == "inbox" {
            return Err(format!(
                "rejected: plan name '{name}' — lowercase letters, digits and '-' (not inbox)"
            ));
        }
        if !matches!(area, "dev" | "marketing") {
            return Err(format!("rejected: --area {area} — dev or marketing"));
        }
        let tx = tx(self)?;
        let n = tx
            .execute(
                "INSERT OR IGNORE INTO plan(app, name, title, area, state, source, truth)
                 VALUES (?1, ?2, ?3, ?4, 'active', '', 'db')",
                params![app, name, title.trim(), area],
            )
            .map_err(err)?;
        if n == 0 {
            return Err(format!(
                "rejected: plan {name} already exists — see `fael plan next`"
            ));
        }
        let pid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO event(chunk_uid, at, by, to_state, plan)
             VALUES (?1, ?2, 'owner', 'active', ?3)",
            params![format!("plan:{app}/{name}"), now, pid],
        )
        .map_err(err)?;
        for (key, title, body) in super::section::SKELETON {
            super::section::put(&tx, pid, key, Some(title), Some(body), now)?;
        }
        tx.commit().map_err(err)
    }

    /// `fael plan park|unpark|done <plan>` on a `truth = db` plan: the owner moves
    /// `plan.state` (an md plan's folder is its state, import sets it). Refused while a
    /// chunk runs — a parked or done plan leaves the board and its agent with it. Returns
    /// the state it left.
    pub fn plan_state(&mut self, id: i64, to: &str, now: &str) -> Result<String, String> {
        let tx = tx(self)?;
        let (app, name, state, truth, running): (String, String, String, String, i64) = tx
            .query_row(
                "SELECT app, name, state, truth,
                        (SELECT COUNT(*) FROM chunk WHERE plan = p.id AND state = 'running')
                 FROM plan p WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(err)?;
        if truth != "db" {
            return Err(format!(
                "rejected: {name}'s chunks live in its md — move the file to .fapony/parked/ or done/ (or `fael plan cutover {name}` first)"
            ));
        }
        let from_ok = match to {
            "active" => state == "parked",
            "parked" => matches!(state.as_str(), "active" | "blocked"),
            _ => state != "done",
        };
        if !from_ok {
            return Err(format!("rejected: {name} is {state}"));
        }
        if running > 0 && to != "active" {
            return Err(format!(
                "rejected: {name} has {running} running chunk(s) — finish or park them first"
            ));
        }
        tx.execute("UPDATE plan SET state = ?1 WHERE id = ?2", params![to, id])
            .map_err(err)?;
        // a plan's own row keys on `plan:<app/name>`, which no chunk uid can be
        tx.execute(
            "INSERT INTO event(chunk_uid, at, by, from_state, to_state, plan)
             VALUES (?1, ?2, 'owner', ?3, ?4, ?5)",
            params![format!("plan:{app}/{name}"), now, state, to, id],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(state)
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
