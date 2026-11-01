//! A bill line's start gate (`wait`): the same `wait_on owner|data` that `chunk start` and
//! `chunk answer` already speak, set at plan time with no run behind it.

use super::State;
use super::agent::unwait;
use super::chunk::{Row, is_date};
use super::store::err;
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};

/// `{on, ask, until?, options?}` — the shape rules of `Store::wait`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Wait {
    on: String,
    ask: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<Vec<String>>,
}

impl Wait {
    /// A chunk's wait columns, `None` when it has none.
    pub(super) fn from_cols(
        on: Option<String>,
        ask: Option<String>,
        until: Option<String>,
        options: Option<String>,
    ) -> Option<Wait> {
        Some(Wait {
            on: on?,
            ask: ask.unwrap_or_default(),
            until,
            options: options.and_then(|o| serde_json::from_str(&o).ok()),
        })
    }

    fn check(&self) -> Result<(), String> {
        if !matches!(self.on.as_str(), "owner" | "data") {
            return Err(format!(
                "rejected: wait.on {} — owner or data; try \"wait\": {{\"on\": \"owner\", \"ask\": \"<question>\"}}",
                self.on
            ));
        }
        if self.ask.trim().is_empty() {
            return Err(
                "rejected: wait needs an ask — the question; try \"ask\": \"<question>\"".into(),
            );
        }
        if self.options.as_ref().is_some_and(|o| !o.is_empty()) && self.on != "owner" {
            return Err("rejected: wait.options needs wait.on owner; try \"on\": \"owner\"".into());
        }
        if self.until.as_deref().is_some_and(|u| !is_date(u)) {
            return Err(
                "rejected: wait.until takes a date YYYY-MM-DD; try \"until\": \"<YYYY-MM-DD>\""
                    .into(),
            );
        }
        if self.on == "data" && self.until.is_none() {
            return Err("rejected: wait.on data needs wait.until YYYY-MM-DD — the date that ends the wait; try \"until\": \"<YYYY-MM-DD>\"".into());
        }
        Ok(())
    }
}

/// Gate chunk `r`: `waiting` with the question, in the bill's own transaction.
pub(super) fn gate(tx: &Transaction, r: &Row, w: &Wait, now: &str) -> Result<String, String> {
    w.check()?;
    if !matches!(r.state, State::Open | State::Draft | State::Waiting) {
        return Err(format!(
            "rejected: chunk {} is {} — a bill gates only a draft or open chunk; try `fael plan export <plan> --json` again without its wait",
            r.uid,
            r.state.as_str()
        ));
    }
    unwait(tx, r.id)?;
    let options = w
        .options
        .as_ref()
        .filter(|o| !o.is_empty())
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE chunk SET state = 'waiting', wait_on = ?1, wait_text = ?2, wait_until = ?3,
           wait_options = ?4 WHERE id = ?5",
        params![w.on, w.ask, w.until, options, r.id],
    )
    .map_err(err)?;
    tx.execute(
        "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
         VALUES (?1, ?2, 'owner', ?3, 'waiting', ?4, (SELECT plan FROM chunk WHERE id = ?5))",
        params![
            r.uid,
            now,
            r.state.as_str(),
            format!("wait {}: {}", w.on, w.ask),
            r.id
        ],
    )
    .map_err(err)?;
    Ok(format!("wait {}", w.on))
}
