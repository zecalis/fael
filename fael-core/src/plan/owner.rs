//! The owner's half (SPEC §1, §5): commands from the board, never fenced, and `run end`.

use super::State;
use super::chunk::{end_live, row, set, tx};
use super::store::{Store, err};
use rusqlite::params;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Answer,
    Accept,
    Drop,
    Park,
    Unpark,
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
            Owner::Answer if !matches!(r.state, State::Waiting | State::Review) => {
                return Err(format!(
                    "rejected: chunk {uid} is {} — `answer` replies to a waiting chunk or sends a review back",
                    r.state.as_str()
                ));
            }
            Owner::Answer | Owner::Unpark => State::Open,
            Owner::Accept => State::Done,
            Owner::Drop => State::Dropped,
            Owner::Park => State::Parked,
        };
        if cmd == Owner::Unpark && r.state != State::Parked {
            return Err(format!(
                "rejected: chunk {uid} is {}, not parked",
                r.state.as_str()
            ));
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
