//! `fael chunk touch <uid> <path>… [--msg "<what you are doing>"]` (plan overlap-broker
//! o3, issue 01M5PB7Z; l3 issue 01M6CZ0K): a running chunk publishes the files it has
//! touched so far. Stored on the live run row (`run.touched`), never inferred from git —
//! only what the agent reports is kept (product:mission, same rule as run-telemetry
//! model/cost), so a waiter sees live files, not the whole declared scope. `--msg`
//! stores the intent beside it (`run.touch_msg`), shown as `doing:` on the other side's
//! avoid card. A new module (not a split: `agent.rs` sits at the 400-line ratchet, so
//! the method lives here instead).

use super::State;
use super::chunk::{Here, fence, row, scope, tx};
use super::store::{Store, err};
use crate::Dodge;
use rusqlite::{OptionalExtension, params};

impl Store {
    /// Publish touched-so-far: each call unions with what is already there, newest
    /// paths last. A bad path rejects the whole call — nothing is stored — so a
    /// typo never narrows what the next brief shows. Fenced to the chunk's live
    /// worktree (or `--run R`), and only while it runs. `--msg` stores the intent
    /// beside `touched` (`run.touch_msg`); `None` keeps what is there.
    pub fn touch(
        &mut self,
        uid: &str,
        files: &[String],
        msg: Option<&str>,
        here: &Here,
    ) -> Result<(), String> {
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
        let want_msg: Option<String> = match msg {
            None => None,
            Some(m) => {
                let t = m.trim();
                if t.is_empty() {
                    return Err(format!(
                        "rejected: --msg needs text — try `fael chunk touch {uid} src/a.rs --msg \"wiring the form\"`"
                    ));
                }
                Some(t.to_string())
            }
        };
        // the live run this call holds: `--run R` names it, else the chunk's live row.
        // `fence` already refused a foreign R, so either names the same live run.
        let cur: (Option<String>, Option<String>) = match here.run {
            Some(run) => tx
                .query_row(
                    "SELECT touched, touch_msg FROM run WHERE chunk = ?1 AND start = ?2 AND ended IS NULL",
                    params![r.id, run],
                    |q| Ok((q.get(0)?, q.get(1)?)),
                )
                .map_err(|_| {
                    format!(
                        "rejected: run {run} is not {uid}'s live run — it ended or never ran it; try `fael chunk show {uid}` to find its live run"
                    )
                })?,
            None => tx
                .query_row(
                    "SELECT touched, touch_msg FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
                    [r.id],
                    |q| Ok((q.get(0)?, q.get(1)?)),
                )
                .optional()
                .map_err(err)?
                .unwrap_or((None, None)),
        };
        let (cur_touched, cur_msg) = cur;
        let mut seen = std::collections::BTreeSet::new();
        let mut merged: Vec<String> = Vec::new();
        for line in cur_touched
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
        let final_msg = want_msg.or(cur_msg);
        let n = match here.run {
            Some(run) => tx
                .execute(
                    "UPDATE run SET touched = ?1, touch_msg = ?2 WHERE chunk = ?3 AND start = ?4 AND ended IS NULL",
                    params![joined, final_msg, r.id, run],
                )
                .map_err(err)?,
            None => tx
                .execute(
                    "UPDATE run SET touched = ?1, touch_msg = ?2 WHERE chunk = ?3 AND ended IS NULL",
                    params![joined, final_msg, r.id],
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

    /// Files `uid` just touched that a live partner also holds — the same
    /// `dodge` the `done` path warns about, surfaced here so the agent learns
    /// it while choosing what to touch, not at `done` when the rebase order
    /// is already set. Warn-only data; the caller prints it, nothing refuses.
    pub fn touch_warnings(
        &self,
        uid: &str,
        files: &[String],
        shared: &[String],
    ) -> Result<Vec<Dodge>, String> {
        let s = scope(files)?;
        let fresh: Vec<String> = s
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        if fresh.is_empty() {
            return Ok(Vec::new());
        }
        self.dodge(uid, &fresh, shared)
    }
}

#[cfg(test)]
mod tests {
    use super::super::chunk::tests::{NOW, start};
    use super::super::{Fields, Store};

    fn scoped(s: &mut Store, title: &str, scope: &[&str]) -> String {
        let f = Fields {
            title: Some(title.into()),
            brief: Some("b".into()),
            scope: Some(scope.iter().map(|x| x.to_string()).collect()),
            ..Fields::default()
        };
        s.add("inbox", &f, &[], NOW).unwrap()
    }

    /// The `done`-time dodge, surfaced at `touch` time: a touched file under
    /// a live partner's scope warns now, not at `done` when rebase order is set.
    #[test]
    fn touch_warnings_names_live_partner_early() {
        let mut s = Store::open_in_memory().unwrap();
        let me = scoped(&mut s, "me", &["me/"]);
        let live = scoped(&mut s, "live", &["src/"]);
        start(&mut s, &live, "/w1").unwrap();
        start(&mut s, &me, "/w2").unwrap();
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let hits = s.touch_warnings(&me, &v(&["src/a.rs"]), &[]).unwrap();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].uid, live);
        let hits = s.touch_warnings(&me, &v(&["other/b.rs"]), &[]).unwrap();
        assert!(hits.is_empty(), "{hits:?}");
        let hits = s
            .touch_warnings(&me, &v(&["src/a.rs"]), &v(&["src/"]))
            .unwrap();
        assert!(hits.is_empty(), "shared: {hits:?}");
    }
}
