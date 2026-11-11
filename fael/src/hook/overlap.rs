//! The edit-time scope overlap line (issue 01M5D9GB): a worktree holding a
//! live chunk run edits a file outside that chunk's declared scope (its
//! pair's with it) that sits under another unfinished chunk's scope — two
//! agents on one file who may not know it. Or the edited file sits inside the
//! holder's own scope but a live partner's run already touched it (f5): same
//! two agents, same line. One line per file, said once per file and session,
//! with the ready `fael chunk note`. The owner label names who (`plan/label`),
//! with its client and `live` vs `stalled Nm`. An unowned file outside
//! scope says nothing (a plain git merge); `chunk done` still lists it.
//! Inform only: no edit is blocked. One read-only SQLite read, no git spawn;
//! `docs/` and `.fael/shared` paths are nobody's, as in `Store::outside`.
//!
//! Scope-only by design (issue 01M6Z008): the real worktree diff is the early
//! warning `fael chunk status` shows as `observed` + `outside declared scope`,
//! before `done`/`run end` claim it. This hook never spawns git.

use super::protocol::Ctx;
use super::say::{Kind, Line};
use crate::core::work;
use std::path::Path;

/// One `Overlap` line per file of the edited files that fit the rule, at most
/// 3 per owner (Threshold trial). Empty with no crew.db, no live run, no
/// declared scope or no overlapping chunk.
pub(crate) fn lines(c: &Ctx, files: &[String]) -> Vec<Line> {
    let db = work::db_for(c.repo.journal.as_deref(), &c.repo.fael);
    at(&db, &c.repo.root, &c.session, files)
}

fn at(db: &Path, root: &Path, session: &str, files: &[String]) -> Vec<Line> {
    if files.is_empty() || !db.exists() {
        return vec![];
    }
    let shared = work::shared(root).unwrap_or_default();
    match work::scopes(db, &root.to_string_lossy(), session, &shared) {
        Some(s) => about(&s, &shared, files),
        None => vec![],
    }
}

/// `live` when `COALESCE(last_seen, started)` is within 30 min, else
/// `stalled Nm` — the board's rule (`holder::live_in_worktree`). Empty when
/// the chunk never ran (no `started`/`last_seen`): declared-scope overlap of
/// a chunk that never ran stays as it was. A stalled run still shows (its
/// files are still changed), labelled.
fn live_label(o: &work::Other) -> String {
    let Some(at) = o.last_seen.as_deref().or(o.since.as_deref()) else {
        return String::new();
    };
    let now = crate::core::now_ms();
    let stale = crate::core::rfc3339(now.saturating_sub(30 * 60 * 1000));
    if at >= stale.as_str() {
        return "live".to_string();
    }
    let mins = match crate::core::ts_ms(at) {
        Some(ms) => ((now as i64 - ms).max(0) / 60_000).max(1),
        None => 1,
    };
    format!("stalled {mins}m")
}

fn about(s: &work::Scopes, shared: &[String], files: &[String]) -> Vec<Line> {
    // Threshold trial: at most 3 lines per owner per push — a big edit still
    // whispers instead of flooding.
    const PER_OWNER_CAP: usize = 3;
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for f in files {
        let hit = s
            .touched_overlap(f, shared)
            .or_else(|| s.scope_overlap(f, shared));
        let Some(o) = hit else {
            continue;
        };
        let n = counts.entry(o.uid.as_str()).or_insert(0);
        if *n >= PER_OWNER_CAP {
            continue;
        }
        *n += 1;
        out.push(Line {
            kind: Kind::Overlap {
                owner: o.uid.clone(),
                file: f.clone(),
            },
            text: short_text(o, f),
        });
    }
    out
}

/// The per-file line: `fael: <file> — <owner> (<client>, <live|stalled Nm>) ·
/// fael chunk note <uid> "…" · yours? fael chunk status`. At most 60 est
/// tokens; `chunk status` already prints the scope retype, so the line
/// stays short.
fn short_text(o: &work::Other, file: &str) -> String {
    let client = o.client.as_deref().unwrap_or("agent");
    let live = live_label(o);
    let live = if live.is_empty() { o.state.clone() } else { live };
    format!(
        "fael: {file} — {} ({client}, {live}) · fael chunk note {} \"<text>\" · yours? fael chunk status\n",
        o.name, o.uid,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::est_tokens;
    use crate::hook::say::Outbox;
    use crate::hook::say_card::At;
    use crate::hook::state::lock_seen;

    fn scopes() -> work::Scopes {
        work::Scopes {
            uid: "U1".into(),
            declared: vec!["src/a/".into()],
            others: vec![work::Other {
                uid: "U2".into(),
                name: "p/b".into(),
                state: "running".into(),
                scope: vec!["src/b/".into()],
                branch: None,
                client: None,
                doing: None,
                since: None,
                last_seen: None,
                touched: Vec::new(),
            }],
        }
    }

    fn live_seen() -> String {
        crate::core::rfc3339(crate::core::now_ms().saturating_sub(2 * 60 * 1000))
    }

    fn stalled_seen() -> String {
        crate::core::rfc3339(crate::core::now_ms().saturating_sub(31 * 60 * 1000))
    }

    fn touched_scopes() -> work::Scopes {
        work::Scopes {
            uid: "U1".into(),
            declared: vec!["src/a/".into()],
            others: vec![work::Other {
                uid: "U2".into(),
                name: "p/b".into(),
                state: "running".into(),
                scope: vec!["src/b/".into()],
                branch: Some("feat-x".into()),
                client: Some("claude".into()),
                doing: Some("wiring auth".into()),
                since: Some("2026-10-11T09:05:00Z".into()),
                last_seen: Some(live_seen()),
                touched: vec!["src/a/x.rs".into()],
            }],
        }
    }

    fn one(f: &str) -> Vec<Line> {
        about(&scopes(), &[], &[f.into()])
    }

    #[test]
    fn scope_overlap_says_short_per_file_line() {
        let ls = one("src/b/x.rs");
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].text,
            "fael: src/b/x.rs — p/b (agent, running) · fael chunk note U2 \"<text>\" · yours? fael chunk status\n"
        );
        assert!(est_tokens(&ls[0].text) <= 60, "{}", ls[0].text);
    }

    #[test]
    fn two_files_under_one_owner_give_two_lines() {
        let ls = about(&scopes(), &[], &["src/b/x.rs".into(), "src/b/y.rs".into()]);
        assert_eq!(ls.len(), 2);
        assert!(ls[0].text.contains("src/b/x.rs"), "{}", ls[0].text);
        assert!(ls[1].text.contains("src/b/y.rs"), "{}", ls[1].text);
        for l in &ls {
            assert!(est_tokens(&l.text) <= 60, "{}", l.text);
        }
    }

    #[test]
    fn unowned_in_scope_docs_and_shared_are_silent() {
        for f in ["src/z.rs", "src/a/x.rs", "docs/b.md"] {
            assert!(one(f).is_empty(), "{f}");
        }
        let shared = ["src/b/".to_string()];
        assert!(about(&scopes(), &shared, &["src/b/x.rs".into()]).is_empty());
    }

    #[test]
    fn no_crew_db_is_silent() {
        let none = std::env::temp_dir().join(format!("fael-overlap-{}", crate::core::ulid()));
        assert!(at(&none.join("crew.db"), &none, "", &["src/b/x.rs".into()]).is_empty());
    }

    #[test]
    fn same_file_says_once_but_another_file_says() {
        let seen = crate::hook::say_contract::seen("s.seen");
        let say = |f: &str| {
            let mut out = Outbox::open(At::Edit, lock_seen(&seen));
            one(f).into_iter().for_each(|l| out.say(l));
            out.reply().context().map(String::from)
        };
        assert!(say("src/b/x.rs").is_some());
        assert_eq!(say("src/b/x.rs"), None, "same file once per session");
        assert!(
            say("src/b/y.rs").is_some(),
            "another file says (once per file)"
        );
    }

    #[test]
    fn a_mixed_edit_gives_one_line_per_file() {
        let s = touched_scopes();
        let ls = about(&s, &[], &["src/a/x.rs".into(), "src/b/y.rs".into()]);
        assert_eq!(ls.len(), 2);
        assert_eq!(
            ls[0].text,
            "fael: src/a/x.rs — p/b (claude, live) · fael chunk note U2 \"<text>\" · yours? fael chunk status\n"
        );
        assert_eq!(
            ls[1].text,
            "fael: src/b/y.rs — p/b (claude, live) · fael chunk note U2 \"<text>\" · yours? fael chunk status\n"
        );
    }

    #[test]
    fn a_touched_file_inside_my_scope_says_short_line() {
        let s = touched_scopes();
        let ls = about(&s, &[], &["src/a/x.rs".into()]);
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].text,
            "fael: src/a/x.rs — p/b (claude, live) · fael chunk note U2 \"<text>\" · yours? fael chunk status\n"
        );
        assert!(est_tokens(&ls[0].text) <= 60, "{}", ls[0].text);
    }

    #[test]
    fn a_stalled_run_shows_stalled_minutes_not_live() {
        let mut s = touched_scopes();
        s.others[0].last_seen = Some(stalled_seen());
        s.others[0].since = Some(stalled_seen());
        let ls = about(&s, &[], &["src/a/x.rs".into()]);
        assert_eq!(ls.len(), 1);
        assert!(
            ls[0].text.contains("stalled 31m"),
            "stalled label: {}",
            ls[0].text
        );
        assert!(
            !ls[0].text.contains("live"),
            "no live for a stalled run: {}",
            ls[0].text
        );
    }

    #[test]
    fn a_live_run_shows_live() {
        let s = touched_scopes();
        let ls = about(&s, &[], &["src/a/x.rs".into()]);
        assert_eq!(ls.len(), 1);
        assert!(ls[0].text.contains("live"), "{}", ls[0].text);
        assert!(!ls[0].text.contains("stalled"), "{}", ls[0].text);
    }

    #[test]
    fn no_partner_touched_is_silent_and_a_second_touched_edit_is_too() {
        assert!(about(&scopes(), &[], &["src/a/x.rs".into()]).is_empty());
        let seen = crate::hook::say_contract::seen("s.seen");
        let say = |s: &work::Scopes, f: &str| {
            let mut out = Outbox::open(At::Edit, lock_seen(&seen));
            about(s, &[], &[f.into()])
                .into_iter()
                .for_each(|l| out.say(l));
            out.reply().context().map(String::from)
        };
        let s = touched_scopes();
        assert!(say(&s, "src/a/x.rs").is_some());
        assert_eq!(say(&s, "src/a/x.rs"), None);
    }
}
