//! `fael chunk touch <uid> <path>…` (plan overlap-broker o3, issue 01M5PB7Z):
//! a running chunk publishes the files it has touched so far. Stored on the live
//! run row (`run.touched`), never inferred from git — only what the agent reports
//! is kept (product:mission, same rule as run-telemetry model/cost), so a waiter
//! sees live files, not the whole declared scope. Split out of `agent.rs`
//! (file-size ratchet).

use super::State;
use super::chunk::{Here, fence, row, scope, tx};
use super::store::{Store, err};
use rusqlite::params;

impl Store {
    /// Publish touched-so-far: each call unions with what is already there, newest
    /// paths last. A bad path rejects the whole call — nothing is stored — so a
    /// typo never narrows what the next brief shows. Fenced to the chunk's live
    /// worktree (or `--run R`), and only while it runs.
    pub fn touch(&mut self, uid: &str, files: &[String], here: &Here) -> Result<(), String> {
        let s = scope(files)?;
        let fresh: Vec<String> = s
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();
        if fresh.is_empty() {
            return Err(
                "rejected: chunk touch needs a path — try `fael chunk touch <uid> src/a.rs`".into(),
            );
        }
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        fence(&tx, &r, here)?;
        if r.state != State::Running {
            return Err(format!(
                "rejected: chunk {uid} is {} — touch needs a running chunk; try `fael chunk start {uid}` first",
                r.state.as_str()
            ));
        }
        // the live run this call holds: `--run R` names it, else the chunk's live row.
        // `fence` already refused a foreign R, so either names the same live run.
        let cur: Option<String> = match here.run {
            Some(run) => tx
                .query_row(
                    "SELECT touched FROM run WHERE chunk = ?1 AND start = ?2 AND ended IS NULL",
                    params![r.id, run],
                    |q| q.get(0),
                )
                .map_err(|_| {
                    format!(
                        "rejected: run {run} is not {uid}'s live run — it ended or never ran it"
                    )
                })?,
            None => tx
                .query_row(
                    "SELECT touched FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
                    [r.id],
                    |q| q.get(0),
                )
                .map_err(err)?,
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut merged: Vec<String> = Vec::new();
        for line in cur
            .unwrap_or_default()
            .lines()
            .chain(fresh.iter().map(String::as_str))
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            if seen.insert(line.to_string()) {
                merged.push(line.to_string());
            }
        }
        let joined = merged.join("\n");
        let n = match here.run {
            Some(run) => tx
                .execute(
                    "UPDATE run SET touched = ?1 WHERE chunk = ?2 AND start = ?3 AND ended IS NULL",
                    params![joined, r.id, run],
                )
                .map_err(err)?,
            None => tx
                .execute(
                    "UPDATE run SET touched = ?1 WHERE chunk = ?2 AND ended IS NULL",
                    params![joined, r.id],
                )
                .map_err(err)?,
        };
        if n == 0 {
            return Err(format!(
                "rejected: chunk {uid} has no live run — try `fael chunk start {uid}` first"
            ));
        }
        tx.commit().map_err(err)
    }
}
