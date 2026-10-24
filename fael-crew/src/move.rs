//! `fael chunk move <uid> --plan <p>` (PLAN-fael-v8): a chunk changes plan and keeps its uid,
//! runs, notes and edges — what merging two plans needs. Only `plan` and `seq` change.

use super::State;
use super::chunk::{row, set, tx};
use super::store::{Store, err};
use rusqlite::{OptionalExtension, params};

impl Store {
    /// One transaction: the chunk goes to the end of `plan` (`name` or `app/name`, a plan of
    /// this db only), an event on the chunk says from where, and one on the old plan moves
    /// its rev (a bill read before the move is stale). Refused for a running chunk, a parked
    /// or done target, and a label the target already has. Returns the old plan's key.
    pub fn move_to(&mut self, uid: &str, plan: &str, now: &str) -> Result<String, String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        if r.state == State::Running {
            return Err(format!(
                "rejected: chunk {} is running — it moves once its run waits, is done or parked",
                r.uid
            ));
        }
        let key = "CASE WHEN app = '' THEN name ELSE app || '/' || name END";
        let (old, old_key, label): (i64, String, Option<String>) = tx
            .query_row(
                &format!(
                    "SELECT p.id, {key}, c.label FROM chunk c JOIN plan p ON p.id = c.plan
                     WHERE c.id = ?1"
                ),
                [r.id],
                |q| Ok((q.get(0)?, q.get(1)?, q.get(2)?)),
            )
            .map_err(err)?;
        let (pid, to_key, state): (i64, String, String) = tx
            .query_row(
                &format!(
                    "SELECT id, {key}, state FROM plan
                     WHERE name = ?1 OR (app <> '' AND app || '/' || name = ?1)
                     ORDER BY app = '' DESC LIMIT 1"
                ),
                [plan],
                |q| Ok((q.get(0)?, q.get(1)?, q.get(2)?)),
            )
            .optional()
            .map_err(err)?
            .ok_or(format!(
                "rejected: no plan '{plan}' in this repo's crew.db — a chunk moves only between plans of one db"
            ))?;
        if pid == old {
            return Err(format!("rejected: chunk {} is already in {to_key}", r.uid));
        }
        if matches!(state.as_str(), "parked" | "done") {
            let back = if state == "parked" {
                "unpark"
            } else {
                "undone"
            };
            return Err(format!(
                "rejected: plan {to_key} is {state} — `fael plan {back} {to_key}` first, or pick an active plan"
            ));
        }
        if let Some(l) = &label {
            let taken: Option<String> = tx
                .query_row(
                    "SELECT uid FROM chunk WHERE plan = ?1 AND label = ?2",
                    params![pid, l],
                    |q| q.get(0),
                )
                .optional()
                .map_err(err)?;
            if let Some(t) = taken {
                return Err(format!(
                    "rejected: label {l} is taken by chunk {t} in {to_key} — rename it first: `fael chunk edit {} --label <new>`",
                    r.uid
                ));
            }
        }
        tx.execute(
            "UPDATE chunk SET plan = ?1,
               seq = (SELECT COALESCE(MAX(seq), 0) + 1024 FROM chunk WHERE plan = ?1)
             WHERE id = ?2",
            params![pid, r.id],
        )
        .map_err(err)?;
        let text = format!("moved from {old_key} to {to_key}");
        set(&tx, &r, r.state, "owner", Some(&text), now)?;
        // the old plan's own row: its chunk list changed, so its rev moves too
        tx.execute(
            "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
             SELECT 'plan:' || app || '/' || name, ?1, 'owner', state, state, ?2, id
             FROM plan WHERE id = ?3",
            params![now, format!("chunk {} moved to {to_key}", r.uid), old],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(old_key)
    }
}

#[cfg(test)]
#[path = "move_tests.rs"]
mod tests;
