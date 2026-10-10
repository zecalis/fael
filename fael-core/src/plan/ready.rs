//! The one `ready` (SPEC §2): `fael chunk start`, the board and the approved queue all ask
//! it, so the board never offers a chunk `start` rejects for any reason but a lost race.
//! Derived on read, never stored — a dated data wait turns ready with no timer.

use super::State;
use super::store::{Store, err};
use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ready {
    Yes,
    /// open (or a data wait past its date) with an `after` not met
    Blocked(Vec<Unmet>),
    /// not startable, and why: "it is running", "it waits on owner", "its plan is parked"
    No(String),
}

/// An `after` not met: the chunk it names (`uid` None = an md ref that names no chunk)
/// and what to show — its label, else its title, else the ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmet {
    pub uid: Option<String>,
    pub what: String,
}

impl Store {
    /// `today` = `YYYY-MM-DD`, compared with `wait_until` as text.
    pub fn ready(&self, uid: &str, today: &str) -> Result<Ready, String> {
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM chunk WHERE uid = ?1", [uid], |r| r.get(0))
            .optional()
            .map_err(err)?
            .ok_or(format!("rejected: no chunk {uid}"))?;
        ready_in(&self.conn, id, today)
    }

    /// The first ready chunk of one plan in plan order, `(uid, title, brief)` — what
    /// `fael kickoff PLAN-x.md` offers. The cross-plan queue (pin, due, unblocks) is b3b's.
    pub fn first_ready(
        &self,
        plan: i64,
        today: &str,
    ) -> Result<Option<(String, String, String)>, String> {
        let mut q = self
            .conn
            .prepare("SELECT id, uid, title, brief FROM chunk WHERE plan = ?1 ORDER BY seq")
            .map_err(err)?;
        let rows: Vec<(i64, String, String, String)> = q
            .query_map([plan], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        for (id, uid, title, brief) in rows {
            if ready_in(&self.conn, id, today)? == Ready::Yes {
                return Ok(Some((uid, title, brief)));
            }
        }
        Ok(None)
    }
}

pub(super) fn ready_in(c: &Connection, id: i64, today: &str) -> Result<Ready, String> {
    let (state, on, until, plan): (String, Option<String>, Option<String>, String) = c
        .query_row(
            "SELECT c.state, c.wait_on, c.wait_until, p.state
             FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(err)?;
    if plan == "parked" {
        return Ok(Ready::No("its plan is parked".into()));
    }
    let due = |u: &Option<String>| u.as_deref().is_some_and(|u| u <= today);
    match (state.as_str(), on.as_deref()) {
        ("open", _) => {}
        ("waiting", Some("data")) if due(&until) => {}
        ("waiting", Some(on)) => {
            let till = until.map(|u| format!(" until {u}")).unwrap_or_default();
            return Ok(Ready::No(format!("it waits on {on}{till}")));
        }
        (s, _) => return Ok(Ready::No(format!("it is {s}"))),
    }
    let mut q = c
        .prepare(
            "SELECT a.uid, COALESCE(a.label, a.title, e.ref, '?'), a.state
             FROM edge e LEFT JOIN chunk a ON a.id = e.dst
             WHERE e.src = ?1 AND e.kind = 'after' ORDER BY a.seq",
        )
        .map_err(err)?;
    let mut unmet = Vec::new();
    for row in q
        .query_map([id], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(err)?
    {
        let (uid, what, st) = row.map_err(err)?;
        if !st
            .as_deref()
            .and_then(State::parse)
            .is_some_and(State::closed)
        {
            unmet.push(Unmet { uid, what });
        }
    }
    Ok(if unmet.is_empty() {
        Ready::Yes
    } else {
        Ready::Blocked(unmet)
    })
}
