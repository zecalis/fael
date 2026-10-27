//! The run reader for `fael stats` flow and hand-off numbers. Read-only,
//! fail-open: no db or no tables gives no runs.

use super::StatsRun;
use super::db::readonly;
use std::path::Path;

/// An agent `done`/`review` event, as the brief's `after` rule reads it.
const AGENT_DONE: &str = "e.by = 'agent' AND e.to_state IN ('done', 'review')";

/// Every run with its chunk's uid, oldest first. Empty when no db or tables.
/// `after_handoff`: the chunk is `after` a chunk whose agent done/review word
/// (non-empty) was written before this run started (the brief's `after` rule
/// plus a time cut). `done`: an agent done/review event inside the run's span.
pub fn flow_runs(db: &Path) -> Vec<StatsRun> {
    let Some(c) = readonly(db) else {
        return Vec::new();
    };
    let sql = format!(
        "SELECT r.started, r.ended, r.pr, r.branch, c.uid, r.session,
           EXISTS(SELECT 1 FROM edge g JOIN chunk d ON d.id = g.dst
                  JOIN event e ON e.chunk_uid = d.uid
                  WHERE g.src = r.chunk AND g.kind = 'after' AND {AGENT_DONE}
                    AND COALESCE(e.text, '') != ''
                    AND julianday(e.at) < julianday(r.started)),
           EXISTS(SELECT 1 FROM event e WHERE e.chunk_uid = c.uid AND {AGENT_DONE}
                    AND r.ended IS NOT NULL
                    AND julianday(e.at) >= julianday(r.started)
                    AND julianday(e.at) <= julianday(r.ended))
         FROM run r JOIN chunk c ON c.id = r.chunk ORDER BY r.id"
    );
    let mut q = match c.prepare(&sql) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };
    q.query_map([], |r| {
        Ok(StatsRun {
            started: r.get(0)?,
            ended: r.get(1)?,
            pr: r.get(2)?,
            branch: r.get(3)?,
            chunk: r.get(4)?,
            session: r.get(5)?,
            after_handoff: r.get(6)?,
            done: r.get(7)?,
        })
    })
    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_handoff_needs_a_word_written_before_the_run_started() {
        let d = std::env::temp_dir().join(format!("fael-runs-{}.db", crate::ulid()));
        let c = rusqlite::Connection::open(&d).unwrap();
        c.execute_batch(
            "CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT);
             CREATE TABLE edge(src INTEGER, dst INTEGER, kind TEXT);
             CREATE TABLE run(id INTEGER PRIMARY KEY, chunk INTEGER, session TEXT,
               started TEXT, ended TEXT, pr INTEGER, branch TEXT);
             CREATE TABLE event(id INTEGER PRIMARY KEY, chunk_uid TEXT, at TEXT,
               by TEXT, to_state TEXT, text TEXT);
             INSERT INTO chunk VALUES (1,'A'),(2,'B');
             INSERT INTO edge VALUES (2,1,'after');
             INSERT INTO event VALUES (1,'A','2026-10-01T00:00:00Z','agent','done','hand-off');
             INSERT INTO event VALUES (2,'B','2026-10-02T01:00:00Z','agent','review','x');
             INSERT INTO run VALUES (1,2,'s','2026-10-02T00:00:00Z','2026-10-02T02:00:00Z',NULL,NULL);
             INSERT INTO run VALUES (2,2,'t','2026-09-30T00:00:00Z','2026-09-30T02:00:00Z',NULL,NULL);
             INSERT INTO run VALUES (3,1,'u','2026-10-03T00:00:00Z',NULL,NULL,NULL);",
        )
        .unwrap();
        drop(c);
        let runs = flow_runs(&d);
        std::fs::remove_file(&d).ok();
        let got: Vec<_> = runs.iter().map(|r| (r.after_handoff, r.done)).collect();
        assert_eq!(got, [(true, true), (false, false), (false, false)]);
    }
}
