//! The brief a start prints and `chunk show` reads: split out of `start.rs` (file-size ratchet).

use super::lookup::{lookup, no_chunk};
use super::section;
use super::start::Brief;
use super::store::{Store, err};
use rusqlite::Transaction;

impl Store {
    /// `fael chunk show <uid|label>`: the brief, owner answers and notes the next start
    /// prints — for a running chunk, the ones its start printed. Read-only, md plans too.
    pub fn show(&self, key: &str) -> Result<Brief, String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        let (id, _, state, ..) = lookup(&tx, key)?.ok_or_else(|| no_chunk(key))?;
        brief(&tx, id, u32::from(state == "running"), None)
    }
}

impl Store {
    /// What `stale_warnings` reads of a chunk — `(title, brief, issues)` — without the rest of
    /// `show` (sections, hand-offs, live overlaps): the board asks it once per open chunk.
    pub fn stale_input(&self, uid: &str) -> Result<(String, String, Vec<String>), String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        let (id, title, brief) = tx
            .query_row(
                "SELECT id, title, brief FROM chunk WHERE uid = ?1",
                [uid],
                |r| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(err)?;
        Ok((title, brief, super::issue::of(&tx, id)?))
    }
}

/// `skip` = how many starts back the answers and notes count from: 1 inside a start (its
/// own event is the newest), 0 for the start still to come. `start` = this start's R:
/// the brief names the model it runs; None = the next start's (latest run, approval, hint).
pub(super) fn brief(
    tx: &Transaction,
    id: i64,
    skip: u32,
    start: Option<&str>,
) -> Result<Brief, String> {
    let (mut b, pid, reads) = tx
        .query_row(
            "SELECT c.uid, c.label, c.title, c.brief, c.size, c.model_hint, c.scope,
                    CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END,
                    p.title, p.id, COALESCE(c.reads, '')
             FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.id = ?1",
            [id],
            |r| {
                let b = Brief {
                    uid: r.get(0)?,
                    label: r.get(1)?,
                    title: r.get(2)?,
                    brief: r.get(3)?,
                    size: r.get(4)?,
                    model: r.get(5)?,
                    scope: r.get(6)?,
                    plan: r.get(7)?,
                    plan_title: r.get(8)?,
                    sections: vec![],
                    reads: vec![],
                    said: vec![],
                    notes: vec![],
                    after: vec![],
                    by_file: vec![],
                    by_file_more: 0,
                    live: vec![],
                    live_more: 0,
                    resume: None,
                    issues: vec![],
                };
                Ok((b, r.get::<_, i64>(9)?, r.get::<_, String>(10)?))
            },
        )
        .map_err(err)?;
    // the model the agent runs: this start's run row, else the latest run's, the
    // approval's, the hint — a `--model` override or approval never prints stale
    b.model = match start {
        Some(r) => tx
            .query_row(
                "SELECT model FROM run WHERE start = ?1 AND chunk = ?2",
                rusqlite::params![r, id],
                |row| row.get(0),
            )
            .map_err(err)?,
        None => tx
            .query_row(
                "SELECT COALESCE((SELECT model FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1),
                                approved_model, model_hint)
                 FROM chunk WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(err)?,
    };
    b.sections = section::list(tx, pid)?;
    b.reads = reads
        .lines()
        .map(|r| Ok((r.to_string(), section::read(tx, pid, r)?)))
        .collect::<Result<_, String>>()?;
    // answers and notes after the start before this one (this start's own event is the
    // newest; a note on a running chunk is not a start)
    let since = |who: &str| {
        format!(
            "SELECT text FROM event
             WHERE chunk_uid = ?1 AND {who} AND text IS NOT NULL
               AND id > COALESCE((SELECT id FROM event WHERE chunk_uid = ?1
                                    AND to_state = 'running' AND by != 'note'
                                  ORDER BY id DESC LIMIT 1 OFFSET {skip}), 0)
             ORDER BY id"
        )
    };
    b.said = texts(
        tx,
        &since("by = 'owner' AND to_state = 'open' AND from_state IN ('waiting', 'review')"),
        &b.uid,
    )?;
    b.notes = texts(tx, &since("by = 'note'"), &b.uid)?;
    b.issues = super::issue::of(tx, id)?;
    b.after = super::handoff::after(tx, id)?;
    (b.by_file, b.by_file_more) = super::handoff::by_file(tx, id, b.scope.as_deref())?;
    (b.live, b.live_more) = super::handoff::live(tx, id, b.scope.as_deref())?;
    Ok(b)
}

fn texts(tx: &Transaction, sql: &str, uid: &str) -> Result<Vec<String>, String> {
    tx.prepare(sql)
        .map_err(err)?
        .query_map([uid], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}
