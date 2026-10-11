//! Contracts between chunks (PLAN-fael-parallel §2, decision fael-parallel:contract): a chunk
//! that needs a file another chunk holds asks for a change with the shape it will code
//! against. The holder agrees or refuses, and its answer is final: no counter, no owner step.
//! An ask to a holder that cannot answer, or one that closes a loop, is refused at once.
//! Every move is one `BEGIN IMMEDIATE` and an `event` row keyed `contract:<id>`; `ask` never
//! ends the asker's run, and fael runs no check — it keeps it.

use super::chunk::{Here, row, tx};
use super::contract_loop::{claimed, loop_path};
use super::store::{Store, err};
use rusqlite::{Transaction, params};
use serde::Serialize;

/// The holder's answer, final.
pub enum Reply<'a> {
    /// `returns` = the shape the holder will deliver, when it differs from the asker's
    Agree {
        note: Option<&'a str>,
        returns: Option<&'a str>,
    },
    Refuse(&'a str),
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
    pub check: Option<String>,
    pub state: String,
    /// why it was refused
    pub reason: Option<String>,
}

#[derive(Clone)]
pub(super) struct Row {
    pub(super) id: String,
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) state: String,
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
    let mut hits: Vec<Row> = q
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

/// A move to `to` with its event, on the asker's plan. `reason` is kept on a refusal.
fn go(
    tx: &Transaction,
    c: &Row,
    to: &str,
    text: &str,
    reason: Option<&str>,
    now: &str,
) -> Result<(), String> {
    tx.execute(
        "UPDATE contract SET state = ?1, reason = COALESCE(?2, reason) WHERE id = ?3",
        params![to, reason, c.id],
    )
    .map_err(err)?;
    let from = (c.state != "new").then_some(c.state.as_str());
    tx.execute(
        "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
         VALUES ('contract:' || ?1, ?2, 'agent', ?3, ?4, ?5,
                 (SELECT plan FROM chunk WHERE id = ?6))",
        params![c.id, now, from, to, text, c.from],
    )
    .map(drop)
    .map_err(err)
}

/// Why `id` cannot answer an ask now: only a running chunk with a live run can.
fn not_live(tx: &Transaction, id: i64, uid: &str) -> Result<Option<String>, String> {
    let state: String = tx
        .query_row("SELECT state FROM chunk WHERE id = ?1", [id], |r| r.get(0))
        .map_err(err)?;
    let live: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM run WHERE chunk = ?1 AND ended IS NULL)",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    Ok(match (state.as_str(), live) {
        ("running", true) => None,
        ("running", false) => Some(format!("chunk {uid}'s run ended — nobody holds its files")),
        (s, _) => Some(format!(
            "chunk {uid} is {s}, not running — nobody is there to answer"
        )),
    })
}

impl Store {
    /// `fael chunk ask <to> "<want>" --returns "<shape>" [--check cmd]`: from the one chunk
    /// this worktree (or `--run R`) runs; its run goes on. Returns the contract id and, when
    /// the ask was refused at once, why: the holder is not running with a live run, or the ask
    /// closes a loop of asks and the asker's claim is the youngest on it (the older claim wins —
    /// when another ask on the loop is from a younger claim, that open ask is refused instead).
    pub fn ask(
        &mut self,
        to: &str,
        want: &str,
        returns: &str,
        check: Option<&str>,
        here: &Here,
    ) -> Result<(String, Option<String>), String> {
        if want.trim().is_empty() || returns.trim().is_empty() {
            return Err("rejected: an ask needs what you want and --returns the shape you will code against".into());
        }
        let tx = tx(self)?;
        let from = match actors(&tx, here)?.as_slice() {
            [one] => *one,
            [] => return Err("rejected: ask comes from a live run — `fael chunk start <uid>` first".into()),
            _ => return Err("rejected: this start holds several chunks — ask from one of them, `--run` cannot tell which".into()),
        };
        let t = row(&tx, to)?;
        if t.id == from {
            return Err("rejected: a chunk cannot ask itself".into());
        }
        let id = crate::ulid();
        let c = Row {
            id: id.clone(),
            from,
            to: t.id,
            state: "new".into(),
        };
        let mut why = not_live(&tx, t.id, &t.uid)?;
        // a loop: the youngest claim among this ask and the open asks on it gives way
        let mut yielded = None;
        if why.is_none()
            && let Some(path) = loop_path(&tx, from, t.id)?
        {
            let mut youngest = (claimed(&tx, from)?, None);
            for e in path.into_iter().filter(|e| e.state == "open") {
                let at = claimed(&tx, e.from)?;
                if at > youngest.0 {
                    youngest = (at, Some(e));
                }
            }
            match youngest.1 {
                None => {
                    why = Some(format!(
                        "closes a loop of asks through chunk {}: the older claim wins",
                        t.uid
                    ));
                }
                Some(e) => {
                    let text = format!(
                        "chunk {} asked {} and closed a loop: the older claim wins",
                        uid(&tx, from)?,
                        t.uid
                    );
                    yielded = Some((e, text));
                }
            }
        }
        tx.execute(
            "INSERT INTO contract(id, from_chunk, to_chunk, want, returns, check_cmd, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'new')",
            params![id, from, t.id, want, returns, check],
        )
        .map_err(err)?;
        match &why {
            Some(w) => go(
                &tx,
                &c,
                "refused",
                &format!("{want} → refused: {w}"),
                Some(w),
                here.now,
            )?,
            None => go(
                &tx,
                &c,
                "open",
                &format!("{want} → {returns}"),
                None,
                here.now,
            )?,
        }
        if let Some((e, text)) = yielded {
            go(&tx, &e, "refused", &text, Some(&text), here.now)?;
        }
        tx.commit().map_err(err)?;
        Ok((id, why))
    }

    /// `fael chunk reply <id> agree [note] [--returns shape] | refuse "<why>"`: the holder,
    /// from its live run, on an open ask. Final. Returns the state it is in now.
    pub fn reply(&mut self, id: &str, r: Reply, here: &Here) -> Result<String, String> {
        let tx = tx(self)?;
        let c = find(&tx, id)?;
        if c.state != "open" {
            return Err(format!(
                "rejected: contract {} is {} — only an open ask is answered, and the answer is final",
                c.id, c.state
            ));
        }
        if !actors(&tx, here)?.contains(&c.to) {
            return Err(format!(
                "rejected: contract {} is open — chunk {} answers it, from its live run's worktree",
                c.id,
                uid(&tx, c.to)?
            ));
        }
        let to = match r {
            Reply::Agree { note, returns } => {
                if let Some(s) = returns {
                    tx.execute(
                        "UPDATE contract SET returns = ?1 WHERE id = ?2",
                        params![s, c.id],
                    )
                    .map_err(err)?;
                }
                go(
                    &tx,
                    &c,
                    "agreed",
                    note.or(returns).unwrap_or("agreed"),
                    None,
                    here.now,
                )?;
                "agreed"
            }
            Reply::Refuse(why) => {
                go(&tx, &c, "refused", why, Some(why), here.now)?;
                "refused"
            }
        };
        tx.commit().map_err(err)?;
        Ok(to.into())
    }

    /// `fael chunk ask withdraw <id>`: the asker takes back an ask nobody has answered yet.
    pub fn withdraw(&mut self, id: &str, here: &Here) -> Result<(), String> {
        let tx = tx(self)?;
        let c = find(&tx, id)?;
        if c.state != "open" {
            return Err(format!(
                "rejected: contract {} is {} — only an open ask is withdrawn",
                c.id, c.state
            ));
        }
        if !actors(&tx, here)?.contains(&c.from) {
            return Err(format!(
                "rejected: contract {} is open — chunk {} asked it and withdraws it, from its live run's worktree",
                c.id,
                uid(&tx, c.from)?
            ));
        }
        go(
            &tx,
            &c,
            "withdrawn",
            "withdrawn by the asker",
            None,
            here.now,
        )?;
        tx.commit().map_err(err)
    }

    /// The asks `chunk` (a uid) holds in and out, newest first.
    pub fn asks(&self, chunk: &str) -> Result<Vec<Ask>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT k.id, f.uid = ?1, IIF(f.uid = ?1, t.uid, f.uid), k.want, k.returns,
                        k.check_cmd, k.state, k.reason
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
                check: r.get(5)?,
                state: r.get(6)?,
                reason: r.get(7)?,
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
