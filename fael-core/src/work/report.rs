//! The Stop/session-start report: what the client says its session runs as and
//! costs so far (PLAN-run-telemetry t3). Crew owns the write side; this is the
//! hook's fail-open half, next to `db::mark_seen` — same predicates, plus the
//! model/cost columns, never `ended`, never the `session` column itself.

use std::path::Path;

/// Report what the client says its session runs as and cost so far (opencode
/// stop/session-start): the live runs of this worktree get the reported model
/// (actual wins over the start-time hint) and cost (last writer wins = session
/// total). Never ends a run, never touches another worktree's rows. An RFC3339
/// hook session is never stored — like `db::mark_seen` it only matches rows
/// that already hold it. Fail-open, never migrates.
pub fn mark_report(
    db: &Path,
    worktree: &str,
    session: &str,
    model: Option<&str>,
    cost: Option<f64>,
) -> usize {
    if !db.exists() || session.is_empty() {
        return 0;
    }
    if model.is_none_or(|m| m.is_empty()) && cost.is_none() {
        return 0;
    }
    // fael never prices: a bad tally is dropped, never stored
    let cost = cost.filter(|c| c.is_finite() && *c >= 0.0);
    if model.is_none_or(|m| m.is_empty()) && cost.is_none() {
        return 0;
    }
    let Ok(c) = rusqlite::Connection::open(db) else {
        return 0;
    };
    c.execute(
        "UPDATE run SET model = COALESCE(?3, model), cost = COALESCE(?4, cost)
         WHERE worktree = ?1 AND ended IS NULL AND (session IS NULL OR session = ?2)",
        rusqlite::params![worktree, session, model, cost],
    )
    .unwrap_or(0)
}

/// [`mark_report`] for the runs a session's `chunk start` bound, wherever the
/// session works now (a `--no-worktree` run has no worktree for `mark_seen`
/// to match). Native sessions only — an RFC3339 hook session is never stored.
pub fn mark_report_session(
    db: &Path,
    session: &str,
    model: Option<&str>,
    cost: Option<f64>,
) -> usize {
    if !db.exists() || session.is_empty() {
        return 0;
    }
    if model.is_none_or(|m| m.is_empty()) && cost.is_none() {
        return 0;
    }
    if crate::ts_ms(session).is_some() {
        return 0;
    }
    // fael never prices: a bad tally is dropped, never stored
    let cost = cost.filter(|c| c.is_finite() && *c >= 0.0);
    if model.is_none_or(|m| m.is_empty()) && cost.is_none() {
        return 0;
    }
    let Ok(c) = rusqlite::Connection::open(db) else {
        return 0;
    };
    c.execute(
        "UPDATE run SET model = COALESCE(?2, model), cost = COALESCE(?3, cost)
         WHERE session = ?1 AND ended IS NULL",
        rusqlite::params![session, model, cost],
    )
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_db() -> (std::path::PathBuf, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("fael-report-{}", crate::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let db = d.join("crew.db");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE run(id INTEGER PRIMARY KEY, start TEXT, chunk INTEGER, client TEXT,
                              session TEXT, model TEXT, worktree TEXT, branch TEXT, pr INTEGER,
                              out TEXT, started TEXT, last_seen TEXT, ended TEXT, cost REAL);",
        )
        .unwrap();
        // live rows in this worktree: one bound to the session, one unbound;
        // one ended (never touched); one live in another worktree
        for (id, start, session, worktree, ended) in [
            (1, "R1", Some("ses_abc"), "/wt", None),
            (2, "R1", None, "/wt", None),
            (
                3,
                "R9",
                Some("ses_abc"),
                "/wt",
                Some("2026-10-24T00:00:00Z"),
            ),
            (4, "R1", Some("ses_abc"), "/other", None),
        ] {
            c.execute(
                "INSERT INTO run(id, start, chunk, session, worktree, ended, started)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, '2026-10-24T00:00:00Z')",
                rusqlite::params![id, start, session, worktree, ended],
            )
            .unwrap();
        }
        drop(c);
        (d, db)
    }

    fn reported(db: &std::path::Path, id: i64) -> (Option<String>, Option<f64>, Option<String>) {
        let c = rusqlite::Connection::open(db).unwrap();
        c.query_row(
            "SELECT model, cost, session FROM run WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    }

    #[test]
    fn report_writes_model_and_cost_on_live_runs_only() {
        let (d, db) = report_db();
        // the session-bound and the unbound live rows in this worktree land it
        assert_eq!(
            mark_report(&db, "/wt", "ses_abc", Some("opencode-go/m"), Some(0.062652)),
            2
        );
        assert_eq!(
            reported(&db, 1),
            (
                Some("opencode-go/m".into()),
                Some(0.062652),
                Some("ses_abc".into())
            )
        );
        assert_eq!(
            reported(&db, 2),
            (Some("opencode-go/m".into()), Some(0.062652), None)
        );
        // ended rows and other worktrees are never touched
        assert_eq!(reported(&db, 3), (None, None, Some("ses_abc".into())));
        assert_eq!(reported(&db, 4), (None, None, Some("ses_abc".into())));
        // last writer wins = session total: a second stop overwrites the cost
        assert_eq!(mark_report(&db, "/wt", "ses_abc", None, Some(0.14646)), 2);
        assert_eq!(reported(&db, 1).1, Some(0.14646));
        // the actual wins over the start-time hint on the next report
        assert_eq!(
            mark_report(&db, "/wt", "ses_abc", Some("opencode-go/n"), None),
            2
        );
        assert_eq!(reported(&db, 1).0.as_deref(), Some("opencode-go/n"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn report_keeps_rfc3339_sessions_out_of_the_table() {
        let (d, db) = report_db();
        let ts = "2026-10-24T07:47:46.457Z";
        // worktree match still lands model/cost, but the timestamp is never stored
        assert_eq!(
            mark_report(&db, "/wt", ts, Some("opencode-go/m"), Some(0.5)),
            1
        );
        assert_eq!(
            reported(&db, 2),
            (Some("opencode-go/m".into()), Some(0.5), None)
        );
        // the session path refuses the timestamp outright
        assert_eq!(
            mark_report_session(&db, ts, Some("opencode-go/m"), Some(0.5)),
            0
        );
        // bad tallies are dropped, never stored — and nothing to say is a no-op
        assert_eq!(mark_report(&db, "/wt", "ses_abc", None, Some(f64::NAN)), 0);
        assert_eq!(mark_report(&db, "/wt", "ses_abc", None, Some(-1.0)), 0);
        assert_eq!(mark_report(&db, "/wt", "ses_abc", None, None), 0);
        assert_eq!(mark_report(&db, "/wt", "", Some("m"), Some(1.0)), 0);
        assert_eq!(mark_report_session(&db, "ses_abc", None, None), 0);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn report_session_reaches_runs_without_a_worktree() {
        let (d, db) = report_db();
        // `--no-worktree` rows match by session alone, from any cwd
        assert_eq!(
            mark_report_session(&db, "ses_abc", Some("opencode-go/m"), Some(1.25)),
            2
        );
        assert_eq!(
            reported(&db, 4),
            (
                Some("opencode-go/m".into()),
                Some(1.25),
                Some("ses_abc".into())
            )
        );
        // the unbound worktree row is not the session's: untouched here
        assert_eq!(reported(&db, 2), (None, None, None));
        let _ = std::fs::remove_dir_all(d);
    }
}
