//! Contracts between chunks (PLAN-fael-parallel §2): a chunk that needs a file another
//! chunk holds asks for a change with the shape it will code against; the holder agrees or
//! counters, once each way, then the owner decides. Every move is one `BEGIN IMMEDIATE` and
//! an `event` row keyed `contract:<id>`. A reply is accepted only from the live run of the
//! chunk whose turn it is; `ask` never ends the asker's run. fael runs no check — it keeps it.

use super::chunk::{Here, row, tx};
use super::store::{Store, err};
use rusqlite::{Transaction, params};
use serde::Serialize;

/// The asks that still bind: a cycle among these is a deadlock.
const LIVE: &str = "('open','countered','agreed','owner')";

/// The reply of the chunk whose turn it is.
pub enum Reply<'a> {
    Agree(Option<&'a str>),
    Counter { shape: &'a str, why: &'a str },
}

/// One ask, as `chunk show` lists it. `dir` is `in` (asked of this chunk) or `out`.
#[derive(Debug, Serialize)]
pub struct Ask {
    pub id: String,
    pub dir: &'static str,
    /// the chunk on the other side
    pub chunk: String,
    pub want: String,
    pub returns: String,
    pub counter: Option<String>,
    pub check: Option<String>,
    pub state: String,
    pub rounds: i64,
}

struct Row {
    id: String,
    from: i64,
    to: i64,
    state: String,
}

/// The chunks with a live run in `here`'s worktree (or start R): who is speaking.
fn actors(tx: &Transaction, here: &Here) -> Result<Vec<i64>, String> {
    let (cond, key) = match here.run {
        Some(r) => ("r.start = ?1", r),
        None => ("r.worktree = ?1", here.worktree),
    };
    tx.prepare(&format!(
        "SELECT r.chunk FROM run r WHERE r.ended IS NULL AND {cond}"
    ))
    .map_err(err)?
    .query_map([key], |r| r.get(0))
    .map_err(err)?
    .collect::<Result<_, _>>()
    .map_err(err)
}

/// A contract by id or unique prefix.
fn find(tx: &Transaction, tok: &str) -> Result<Row, String> {
    let mut q = tx
        .prepare("SELECT id, from_chunk, to_chunk, state FROM contract WHERE id LIKE ?1 || '%'")
        .map_err(err)?;
    let hits: Vec<Row> = q
        .query_map([tok.to_uppercase()], |r| {
            Ok(Row {
                id: r.get(0)?,
                from: r.get(1)?,
                to: r.get(2)?,
                state: r.get(3)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut hits = hits;
    match hits.len() {
        1 => Ok(hits.remove(0)),
        0 => Err(format!(
            "rejected: no contract {tok} — `fael chunk show` lists a chunk's asks"
        )),
        _ => Err(format!(
            "rejected: contract {tok} is ambiguous — use more of the id"
        )),
    }
}

fn uid(tx: &Transaction, id: i64) -> Result<String, String> {
    tx.query_row("SELECT uid FROM chunk WHERE id = ?1", [id], |r| r.get(0))
        .map_err(err)
}

/// The event of a move, on the asker's plan.
fn event(tx: &Transaction, c: &Row, to: &str, text: &str, now: &str) -> Result<(), String> {
    let from = (c.state != to).then_some(c.state.as_str());
    tx.execute(
        "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
         VALUES ('contract:' || ?1, ?2, 'agent', ?3, ?4, ?5,
                 (SELECT plan FROM chunk WHERE id = ?6))",
        params![c.id, now, from, to, text, c.from],
    )
    .map(drop)
    .map_err(err)
}

/// The side whose turn it is must be speaking from its live run.
fn turn(tx: &Transaction, c: &Row, who: i64, here: &Here) -> Result<(), String> {
    if actors(tx, here)?.contains(&who) {
        return Ok(());
    }
    Err(format!(
        "rejected: contract {} is {} — chunk {} answers it, from its live run's worktree",
        c.id,
        c.state,
        uid(tx, who)?
    ))
}

impl Store {
    /// `fael chunk ask <to> "<want>" --returns "<shape>" [--check cmd]`: from the one chunk
    /// this worktree (or `--run R`) runs. Its run goes on. Refused for itself, for a chunk
    /// that is over, and when `to` already asks (through other asks) for something of this one.
    pub fn ask(
        &mut self,
        to: &str,
        want: &str,
        returns: &str,
        check: Option<&str>,
        here: &Here,
    ) -> Result<String, String> {
        if want.trim().is_empty() || returns.trim().is_empty() {
            return Err("rejected: an ask needs what you want and --returns the shape you will code against".into());
        }
        let tx = tx(self)?;
        let held = actors(&tx, here)?;
        let from = match held.as_slice() {
            [one] => *one,
            [] => return Err("rejected: ask comes from a live run — `fael chunk start <uid>` first".into()),
            _ => return Err("rejected: this start holds several chunks — ask from one of them, `--run` cannot tell which".into()),
        };
        let t = row(&tx, to)?;
        if t.id == from {
            return Err("rejected: a chunk cannot ask itself".into());
        }
        if t.state.terminal() {
            return Err(format!(
                "rejected: chunk {} is {} — ask a chunk still to run, or `fael chunk add` the change",
                t.uid,
                t.state.as_str()
            ));
        }
        let cycle: bool = tx
            .query_row(
                &format!(
                    "WITH RECURSIVE w(id) AS (SELECT ?1 UNION SELECT c.to_chunk FROM contract c
                       JOIN w ON c.from_chunk = w.id WHERE c.state IN {LIVE})
                     SELECT EXISTS(SELECT 1 FROM w WHERE id = ?2)"
                ),
                params![t.id, from],
                |r| r.get(0),
            )
            .map_err(err)?;
        if cycle {
            return Err(format!(
                "rejected: chunk {} already waits on this one through open asks — an ask here closes a loop",
                t.uid
            ));
        }
        let id = crate::ulid();
        tx.execute(
            "INSERT INTO contract(id, from_chunk, to_chunk, want, returns, check_cmd, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open')",
            params![id, from, t.id, want, returns, check],
        )
        .map_err(err)?;
        let c = Row {
            id: id.clone(),
            from,
            to: t.id,
            state: "open".into(),
        };
        event(&tx, &c, "open", &format!("{want} → {returns}"), here.now)?;
        tx.commit().map_err(err)?;
        Ok(id)
    }

    /// `fael chunk reply <id> agree [note] | counter "<shape>" "<why>"`. The holder answers
    /// an open ask; the asker answers its counter — agree takes the counter's shape, a
    /// second counter goes to the owner. Returns the state the contract is in now.
    pub fn reply(&mut self, id: &str, r: Reply, here: &Here) -> Result<String, String> {
        let tx = tx(self)?;
        let c = find(&tx, id)?;
        let (to, who) = match (c.state.as_str(), &r) {
            ("open", Reply::Agree(_)) => ("agreed", c.to),
            ("open", Reply::Counter { .. }) => ("countered", c.to),
            ("countered", Reply::Agree(_)) => ("agreed", c.from),
            ("countered", Reply::Counter { .. }) => ("owner", c.from),
            ("owner", _) => {
                return Err(format!(
                    "rejected: contract {} waits on the owner — `fael chunk answer {} \"<shape>\"`",
                    c.id, c.id
                ));
            }
            (s, _) => {
                return Err(format!(
                    "rejected: contract {} is {s} — no reply takes it",
                    c.id
                ));
            }
        };
        turn(&tx, &c, who, here)?;
        let (text, sql) = match r {
            Reply::Agree(note) => (
                note.unwrap_or("agreed").to_string(),
                // the asker's agree takes the holder's counter shape
                "UPDATE contract SET state = ?1, returns = COALESCE(counter, returns) WHERE id = ?2",
            ),
            Reply::Counter { shape, why } => {
                // the holder's shape sits in `counter`, the asker's second try in `returns`
                let col = if c.state == "open" {
                    "counter"
                } else {
                    "returns"
                };
                tx.execute(
                    &format!("UPDATE contract SET {col} = ?1, rounds = rounds + 1 WHERE id = ?2"),
                    params![shape, c.id],
                )
                .map_err(err)?;
                (
                    format!("{shape} — {why}"),
                    "UPDATE contract SET state = ?1 WHERE id = ?2",
                )
            }
        };
        tx.execute(sql, params![to, c.id]).map_err(err)?;
        event(&tx, &c, to, &text, here.now)?;
        tx.commit().map_err(err)?;
        Ok(to.into())
    }

    /// `fael chunk ask withdraw <id>`: the asker takes back an ask nobody has agreed yet.
    pub fn withdraw(&mut self, id: &str, here: &Here) -> Result<(), String> {
        let tx = tx(self)?;
        let c = find(&tx, id)?;
        if !matches!(c.state.as_str(), "open" | "countered" | "owner") {
            return Err(format!(
                "rejected: contract {} is {} — only an ask not yet agreed is withdrawn",
                c.id, c.state
            ));
        }
        turn(&tx, &c, c.from, here)?;
        tx.execute(
            "UPDATE contract SET state = 'withdrawn' WHERE id = ?1",
            [&c.id],
        )
        .map_err(err)?;
        event(&tx, &c, "withdrawn", "withdrawn by the asker", here.now)?;
        tx.commit().map_err(err)
    }

    /// Is `tok` a contract id (or a prefix of one)? `chunk answer` tries chunks first.
    pub fn is_contract(&self, tok: &str) -> bool {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM contract WHERE id LIKE ?1 || '%')",
                [tok.to_uppercase()],
                |r| r.get(0),
            )
            .unwrap_or(false)
    }

    /// `fael chunk answer <id> "<shape>"`: the owner settles a contract the two chunks
    /// could not — the shape both code against.
    pub fn answer_contract(&mut self, id: &str, shape: &str, now: &str) -> Result<(), String> {
        let tx = tx(self)?;
        let c = find(&tx, id)?;
        if c.state != "owner" {
            return Err(format!(
                "rejected: contract {} is {} — `answer` settles one that waits on the owner",
                c.id, c.state
            ));
        }
        tx.execute(
            "UPDATE contract SET state = 'agreed', returns = ?1 WHERE id = ?2",
            params![shape, c.id],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
             VALUES ('contract:' || ?1, ?2, 'owner', 'owner', 'agreed', ?3,
                     (SELECT plan FROM chunk WHERE id = ?4))",
            params![c.id, now, shape, c.from],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    /// The asks `chunk` (a uid) holds in and out, newest first.
    pub fn asks(&self, chunk: &str) -> Result<Vec<Ask>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT k.id, f.uid = ?1, IIF(f.uid = ?1, t.uid, f.uid), k.want, k.returns,
                        k.counter, k.check_cmd, k.state, k.rounds
                 FROM contract k JOIN chunk f ON f.id = k.from_chunk JOIN chunk t ON t.id = k.to_chunk
                 WHERE f.uid = ?1 OR t.uid = ?1 ORDER BY k.id DESC",
            )
            .map_err(err)?;
        q.query_map([chunk], |r| {
            Ok(Ask {
                id: r.get(0)?,
                dir: if r.get(1)? { "out" } else { "in" },
                chunk: r.get(2)?,
                want: r.get(3)?,
                returns: r.get(4)?,
                counter: r.get(5)?,
                check: r.get(6)?,
                state: r.get(7)?,
                rounds: r.get(8)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;
