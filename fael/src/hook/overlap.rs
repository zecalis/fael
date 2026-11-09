//! The edit-time scope overlap line (issue 01M5D9GB): a worktree holding a
//! live chunk run edits a file outside that chunk's declared scope (its
//! pair's with it) that sits under another unfinished chunk's scope — two
//! agents on one file who may not know it. Or the edited file sits inside the
//! holder's own scope but a live partner's run already touched it (f5): same
//! two agents, same line. One line per owner chunk (every edited file of it
//! listed), said once per owner chunk and session, with the ready
//! `fael chunk note`, and the owner's state so a running chunk reads
//! differently from one nobody started. The touched half names the partner's
//! branch, what it said it is doing and since when. An unowned file outside
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

/// One `Overlap` line per owner chunk of the edited files that fit the rule. Empty with no
/// crew.db, no live run, no declared scope or no overlapping chunk.
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

fn about(s: &work::Scopes, shared: &[String], files: &[String]) -> Vec<Line> {
    let mut by: Vec<(&work::Other, Vec<&str>, Vec<&str>)> = vec![];
    for f in files {
        let hit: Option<(&work::Other, bool)> = match s.touched_overlap(f, shared) {
            Some(o) => Some((o, true)),
            None => s.scope_overlap(f, shared).map(|o| (o, false)),
        };
        let Some((o, touched)) = hit else {
            continue;
        };
        match by.iter_mut().find(|(x, _, _)| x.uid == o.uid) {
            Some((_, tfs, sfs)) => {
                if touched {
                    tfs.push(f);
                } else {
                    sfs.push(f);
                }
            }
            None => by.push(if touched {
                (o, vec![f], vec![])
            } else {
                (o, vec![], vec![f])
            }),
        }
    }
    by.into_iter()
        .map(|(o, tfs, sfs)| {
            if sfs.is_empty() {
                return Line {
                    kind: Kind::Overlap { owner: o.uid.clone() },
                    text: touched_text(o, &tfs),
                };
            }
            // the scope files are this chunk's work? widen its scope: a superset keeps the approval
            let wider: Vec<&str> = s.declared.iter().map(String::as_str).chain(sfs.iter().copied()).collect();
            let scope = format!(
                "{} outside chunk {}'s scope, inside {} chunk {}'s",
                sfs.join(", "),
                s.uid,
                o.state,
                o.name
            );
            let text = if tfs.is_empty() {
                scope
            } else {
                format!("{} touched by live {} chunk {}'s{} + {scope}", tfs.join(", "), o.state, o.name, detail(o))
            };
            Line {
                kind: Kind::Overlap { owner: o.uid.clone() },
                text: format!(
                    "fael: {text} — coordinate: fael chunk note {} \"<text>\" · yours? fael chunk edit {} --scope {} (a superset keeps approval) · or go on; said once per chunk, chunk done lists every file\n",
                    o.uid,
                    s.uid,
                    wider.join(",")
                ),
            }
        })
        .collect()
}

/// The partner's live-run detail: its branch, what it said it is doing and
/// since when. Only present parts are named, empty when there are none.
fn detail(o: &work::Other) -> String {
    let mut parts = vec![];
    if let Some(b) = o.branch.as_deref() {
        parts.push(format!("on {b}"));
    }
    if let Some(d) = o.doing.as_deref() {
        parts.push(format!("doing {d}"));
    }
    if let Some(since) = o.since.as_deref() {
        parts.push(format!("since {since}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(" · "))
    }
}

/// The touched-only line: the partner's live files, with its branch, what
/// it said it is doing and since when.
fn touched_text(o: &work::Other, fs: &[&str]) -> String {
    format!(
        "fael: {} touched by live {} chunk {}'s{} — coordinate: fael chunk note {} \"<text>\" · or go on; said once per chunk, chunk done lists every file\n",
        fs.join(", "),
        o.state,
        o.name,
        detail(o),
        o.uid,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
                doing: None,
                since: None,
                touched: Vec::new(),
            }],
        }
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
                doing: Some("wiring auth".into()),
                since: Some("2026-10-11T09:05:00Z".into()),
                touched: vec!["src/a/x.rs".into()],
            }],
        }
    }

    fn one(f: &str) -> Vec<Line> {
        about(&scopes(), &[], &[f.into()])
    }

    #[test]
    fn overlapping_open_chunk_says_one_line() {
        let ls = one("src/b/x.rs");
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].text,
            "fael: src/b/x.rs outside chunk U1's scope, inside running chunk p/b's — coordinate: fael chunk note U2 \"<text>\" · yours? fael chunk edit U1 --scope src/a/,src/b/x.rs (a superset keeps approval) · or go on; said once per chunk, chunk done lists every file\n"
        );
    }

    #[test]
    fn two_files_of_one_owner_in_one_edit_say_one_line() {
        let ls = about(&scopes(), &[], &["src/b/x.rs".into(), "src/b/y.rs".into()]);
        assert_eq!(ls.len(), 1);
        assert!(
            ls[0]
                .text
                .starts_with("fael: src/b/x.rs, src/b/y.rs outside chunk U1's")
        );
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
    fn a_second_edit_of_the_same_owner_is_silent() {
        let seen = crate::hook::say_contract::seen("s.seen");
        let say = |f: &str| {
            let mut out = Outbox::open(At::Edit, lock_seen(&seen));
            one(f).into_iter().for_each(|l| out.say(l));
            out.reply().context().map(String::from)
        };
        assert!(say("src/b/x.rs").is_some());
        assert_eq!(say("src/b/x.rs"), None);
        assert_eq!(say("src/b/y.rs"), None, "another file, same owner chunk");
    }

    #[test]
    fn a_mixed_edit_names_each_file_for_what_it_is() {
        let ls = about(
            &touched_scopes(),
            &[],
            &["src/a/x.rs".into(), "src/b/y.rs".into()],
        );
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].text,
            "fael: src/a/x.rs touched by live running chunk p/b's (on feat-x · doing wiring auth · since 2026-10-11T09:05:00Z) + src/b/y.rs outside chunk U1's scope, inside running chunk p/b's — coordinate: fael chunk note U2 \"<text>\" · yours? fael chunk edit U1 --scope src/a/,src/b/y.rs (a superset keeps approval) · or go on; said once per chunk, chunk done lists every file\n"
        );
    }

    #[test]
    fn a_touched_file_inside_my_scope_says_branch_doing_and_since() {
        let ls = about(&touched_scopes(), &[], &["src/a/x.rs".into()]);
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].text,
            "fael: src/a/x.rs touched by live running chunk p/b's (on feat-x · doing wiring auth · since 2026-10-11T09:05:00Z) — coordinate: fael chunk note U2 \"<text>\" · or go on; said once per chunk, chunk done lists every file\n"
        );
    }

    #[test]
    fn a_touched_line_missing_parts_still_names_its_note() {
        let mut s = touched_scopes();
        let o = &mut s.others[0];
        o.branch = None;
        o.doing = None;
        o.since = None;
        let ls = about(&s, &[], &["src/a/x.rs".into()]);
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].text,
            "fael: src/a/x.rs touched by live running chunk p/b's — coordinate: fael chunk note U2 \"<text>\" · or go on; said once per chunk, chunk done lists every file\n"
        );
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
        assert_eq!(say(&s, "src/a/x.rs"), None, "same touched file, same owner");
    }
}
