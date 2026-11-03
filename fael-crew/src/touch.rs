//! `fael chunk touch <uid> <path>… [--msg "<what you are doing>"]` (plan overlap-broker
//! o3, issue 01M5PB7Z; l3 issue 01M6CZ0K): a running chunk publishes the files it has
//! touched so far. Stored on the live run row (`run.touched`), never inferred from git —
//! only what the agent reports is kept (product:mission, same rule as run-telemetry
//! model/cost), so a waiter sees live files, not the whole declared scope. `--msg`
//! stores the intent beside it (`run.touch_msg`), shown as `doing:` on the other side's
//! avoid card. A new module (not a split: `agent.rs` sits at the 400-line ratchet, so
//! the method lives here instead).

use super::State;
use super::chunk::{Here, Row, fence, row, scope, set, tx};
use super::store::{Store, err};
use crate::Dodge;
use rusqlite::{OptionalExtension, Transaction, params};

/// Event text for a `--status` snapshot: tied to the run start R that reported
/// it, so a lost-hunk audit reads who left what unstaged when. Empty (a clean
/// tree) reads `clean`, never a blank event. Capped, never rejected: a dirty
/// tree is a fact, not a bad call.
fn status_text(run: &str, status: &str) -> String {
    const CAP: usize = 8000;
    if status.trim().is_empty() {
        return format!("touch {run} status: clean");
    }
    let mut s = status.to_string();
    if s.len() > CAP {
        // byte 8000 can split a multi-byte filename: back to a char boundary
        // or truncate panics and the whole touch rolls back (01M6H01152 review).
        let mut cut = CAP;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n…(truncated)");
    }
    format!("touch {run} status:\n{s}")
}

/// The live run this call holds: `--run R` names it, else the chunk's live row.
/// The start id rides along for the `--status` event text (who, when). `fence`
/// already refused a foreign R, so either names the same live run.
fn live_row(
    tx: &Transaction,
    r: &Row,
    uid: &str,
    here: &Here,
) -> Result<(String, Option<String>, Option<String>), String> {
    match here.run {
        Some(run) => {
            let (touched, msg): (Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT touched, touch_msg FROM run WHERE chunk = ?1 AND start = ?2 AND ended IS NULL",
                    params![r.id, run],
                    |q| Ok((q.get(0)?, q.get(1)?)),
                )
                .map_err(|_| {
                    format!(
                        "rejected: run {run} is not {uid}'s live run — it ended or never ran it; try `fael chunk show {uid}` to find its live run"
                    )
                })?;
            Ok((run.to_string(), touched, msg))
        }
        None => Ok(tx
            .query_row(
                "SELECT start, touched, touch_msg FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
                [r.id],
                |q| Ok((q.get(0)?, q.get(1)?, q.get(2)?)),
            )
            .optional()
            .map_err(err)?
            .unwrap_or_default()),
    }
}

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
        self.touch_with_status(uid, files, msg, None, here)
    }

    /// `touch` plus `--status`: the agent's own `git status --porcelain` output,
    /// stored in the touch event text tied to the run id (issue 01M6HA52P) — the
    /// audit a lost-hunk incident replays. Agent-reported only: fael never spawns
    /// git here. `None` (no flag) writes no event, keeping old calls quiet.
    pub fn touch_with_status(
        &mut self,
        uid: &str,
        files: &[String],
        msg: Option<&str>,
        status: Option<&str>,
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
        // The start id rides along for the `--status` event text (who, when).
        let (run_start, cur_touched, cur_msg) = live_row(&tx, &r, uid, here)?;
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
        // `--status`: the agent's snapshot rides the touch event, tied to the run
        // that reported it — the audit a lost-hunk replay reads. No flag, no event:
        // old calls stay quiet and old event counts hold.
        if let Some(st) = status {
            set(
                &tx,
                &r,
                r.state,
                "agent",
                Some(&status_text(&run_start, st)),
                here.now,
            )?;
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
    use super::super::chunk::tests::{NOW, here, start};
    use super::super::{Fields, Store};
    use super::status_text;

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

    fn live_run(s: &Store, uid: &str) -> String {
        s.conn
            .query_row(
                "SELECT start FROM run WHERE chunk = (SELECT id FROM chunk WHERE uid = ?1) AND ended IS NULL",
                [uid],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn last_touch_event(s: &Store, uid: &str) -> String {
        s.conn
            .query_row(
                "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC LIMIT 1",
                [uid],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// f6 `--status`: the agent's snapshot is stored in the touch event text,
    /// tied to the run id that reported it — the lost-hunk audit replays it.
    #[test]
    fn touch_status_is_stored_in_event_tied_to_run() {
        let mut s = Store::open_in_memory().unwrap();
        let a = scoped(&mut s, "a", &["src/"]);
        start(&mut s, &a, "/wa").unwrap();
        let run = live_run(&s, &a);
        s.touch_with_status(
            &a,
            &["src/a.rs".to_string()],
            None,
            Some(" M src/a.rs\n?? src/new.rs"),
            &here("/wa"),
        )
        .unwrap();
        let ev = last_touch_event(&s, &a);
        assert!(ev.contains(&run), "tied to the run: {ev}");
        assert!(ev.contains(" M src/a.rs"), "snapshot kept whole: {ev}");
        assert!(ev.contains("?? src/new.rs"), "untracked kept: {ev}");
    }

    /// f6: no flag, no event — old calls stay quiet and old event counts hold.
    #[test]
    fn touch_without_status_writes_no_event() {
        let mut s = Store::open_in_memory().unwrap();
        let a = scoped(&mut s, "a", &["src/"]);
        start(&mut s, &a, "/wa").unwrap();
        let n: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))
            .unwrap();
        s.touch(&a, &["src/a.rs".to_string()], None, &here("/wa"))
            .unwrap();
        let m: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))
            .unwrap();
        assert_eq!((n, m), (n, n), "no event without --status");
    }

    /// f6: a clean tree (`git status --porcelain` prints nothing) audits as
    /// clean — never a blank event, never a reject.
    #[test]
    fn touch_status_clean_marks_clean() {
        let mut s = Store::open_in_memory().unwrap();
        let a = scoped(&mut s, "a", &["src/"]);
        start(&mut s, &a, "/wa").unwrap();
        for empty in ["", "   ", "\n"] {
            s.touch_with_status(
                &a,
                &["src/a.rs".to_string()],
                None,
                Some(empty),
                &here("/wa"),
            )
            .unwrap();
            let ev = last_touch_event(&s, &a);
            assert!(ev.contains("clean"), "clean tree: {ev:?}");
        }
    }

    /// f6: the snapshot is a fact, not a bad call — a huge tree truncates
    /// instead of rejecting, so the audit keeps the head.
    #[test]
    fn touch_status_text_truncates_huge_snapshot() {
        let big = " M a.rs\n".repeat(2000);
        let t = status_text("R1", &big);
        assert!(t.starts_with("touch R1 status:\n"), "{t:?}");
        assert!(t.contains("…(truncated)"), "marked");
        assert!(t.len() < big.len(), "capped");
        assert_eq!(status_text("R1", ""), "touch R1 status: clean");
        // a multi-byte char straddling the cap must not panic the touch:
        // byte 8000 lands inside 'é' here, so the cut backs to its start.
        let straddle = format!("{}éb", "a".repeat(7999));
        let t = status_text("R1", &straddle);
        assert!(t.contains("…(truncated)"), "marked, never panics");
        // one ending fully before the cap is kept whole, never split.
        let whole = format!("{}é{}", "a".repeat(7998), "b".repeat(100));
        let t = status_text("R1", &whole);
        assert!(t.contains('é'), "kept whole: {t:?}");
    }
}
