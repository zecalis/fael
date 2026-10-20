//! Ready math for the seam: which chunk `kickoff` offers next.

use super::db::{all_plans, held, readonly};
use super::pure::rules;
use std::path::Path;

pub fn ready_in(c: &rusqlite::Connection, id: i64, today: &str) -> Option<bool> {
    let (state, on, until, plan): (String, Option<String>, Option<String>, String) = c
        .query_row(
            "SELECT c.state, c.wait_on, c.wait_until, p.state
             FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()?;
    if plan == "parked" {
        return Some(false);
    }
    let due = |u: &Option<String>| u.as_deref().is_some_and(|u| u <= today);
    match (state.as_str(), on.as_deref()) {
        ("open", _) => {}
        ("waiting", Some("data")) if due(&until) => {}
        ("waiting", Some(_)) => return Some(false),
        (s, _) if s != "open" => return Some(false),
        _ => {}
    }
    let mut q = c
        .prepare(
            "SELECT a.uid, COALESCE(a.label, a.title, e.ref, '?'), a.state
             FROM edge e LEFT JOIN chunk a ON a.id = e.dst
             WHERE e.src = ?1 AND e.kind = 'after' ORDER BY a.seq",
        )
        .ok()?;
    let rows = q
        .query_map([id], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    for (_, _, st) in rows {
        if !st.as_deref().is_some_and(|s| s == "done" || s == "dropped") {
            return Some(false);
        }
    }
    Some(true)
}

/// The first ready chunk of one plan in plan order, `(uid, title, brief)`.
pub fn first_ready(db: &Path, plan: i64, today: &str) -> Option<(String, String, String)> {
    let c = readonly(db)?;
    let mut q = c
        .prepare("SELECT id, uid, title, brief FROM chunk WHERE plan = ?1 ORDER BY seq")
        .ok()?;
    let rows = q
        .query_map([plan], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    for (id, uid, title, brief) in rows {
        if ready_in(&c, id, today) == Some(true) {
            return Some((uid, title, brief));
        }
    }
    None
}

/// What `fael kickoff PLAN-x.md` prints for plan `x`.
pub fn kickoff_text(db: &Path, plan_name: &str, worktree: &str, today: &str) -> Option<String> {
    let c = readonly(db)?;
    let p = all_plans(&c)?.into_iter().find(|p| p.name == plan_name)?;
    let mut out = String::new();
    let h = held(db, worktree);
    if !h.is_empty() {
        for (uid, title, run) in &h {
            out.push_str(&format!("you hold chunk {uid} — {title} (run {run})\n"));
        }
        out.push_str(&rules(&h[0].0));
        return Some(out);
    }
    match first_ready(db, p.id, today) {
        Some((uid, title, brief)) => {
            out.push_str(&format!("next chunk: {uid} — {title}\n"));
            if brief != title {
                out.push_str(&format!("> {}\n", brief.lines().next().unwrap_or_default()));
            }
            out.push_str(
                "start it: `fael chunk start {uid}` (prints the brief and the chunk rules)\n\n"
                    .replace("{uid}", &uid)
                    .as_str(),
            );
            Some(out)
        }
        None => {
            let mut q = c
                .prepare("SELECT COUNT(*) FROM chunk WHERE plan = ?1")
                .ok()?;
            let n: i64 = q.query_row([p.id], |r| r.get(0)).ok()?;
            if n == 0 {
                return None;
            }
            Some(format!(
                "no chunk of {} is ready — `fael plan export {}` shows each state\n\n",
                p.name, p.name
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn missing_db_reads_empty_and_writes_zero() {
        let db = std::env::temp_dir().join(format!("fael-work-{}.db", crate::ulid()));
        let _ = std::fs::remove_file(&db);
        assert!(held(&db, "/tmp/wt").is_empty());
        assert!(!has_held(&db, "/tmp/wt"));
        assert!(!is_chunk(&db, "01M5AF5WHVBRJ9629ZYA2HF9XY"));
        assert!(chunk_brief(&db, "01M5AF5WHVBRJ9629ZYA2HF9XY").is_none());
        assert!(ended_runs(&db).is_empty());
        assert!(flow_runs(&db).is_empty());
        assert!(plan_by_name(&db, "x").is_none());
        assert!(first_ready(&db, 1, "2026-10-19").is_none());
        assert!(kickoff_text(&db, "x", "/tmp/wt", "2026-10-19").is_none());
        assert_eq!(mark_seen(&db, "/tmp/wt", "s", "2026-10-19T00:00:00Z"), 0);
        assert_eq!(mark_seen_session(&db, "s", "2026-10-19T00:00:00Z"), 0);
        assert!(overlap(&["src/".to_string()], &["src/a.rs".to_string()]));
        assert!(!overlap(&["src/a".to_string()], &["src/ab.rs".to_string()]));
        assert!(held_session_file("abc-123").is_some());
        assert!(held_session_file("../x").is_none());
    }
}
