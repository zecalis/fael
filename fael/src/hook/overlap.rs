//! The edit-time scope overlap line (issue 01M5D9GB): a worktree holding a
//! live chunk run edits a file outside that chunk's declared scope (its
//! pair's with it) that sits under another unfinished chunk's scope — two
//! agents on one file who may not know it. One line per owner chunk (every
//! edited file of it listed), said once per owner chunk and session, with the
//! ready `fael chunk note`, and the owner's state so a running chunk reads
//! differently from one nobody started. An unowned file outside scope says
//! nothing (a plain git merge); `chunk done` still lists it. Inform only: no
//! edit is blocked. One read-only SQLite read, no git spawn; `docs/` and
//! `.fael/shared` paths are nobody's, as in `Store::outside`.

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
    let mut by: Vec<(&work::Other, Vec<&str>)> = vec![];
    for f in files {
        let Some(o) = s.overlap(f, shared) else {
            continue;
        };
        match by.iter_mut().find(|(x, _)| x.uid == o.uid) {
            Some((_, fs)) => fs.push(f),
            None => by.push((o, vec![f])),
        }
    }
    by.into_iter()
        .map(|(o, fs)| Line {
            kind: Kind::Overlap { owner: o.uid.clone() },
            text: format!(
                "fael: {} outside chunk {}'s scope, inside {} chunk {}'s — coordinate: fael chunk note {} \"<text>\", or go on; said once per chunk, chunk done lists every file\n",
                fs.join(", "),
                s.uid,
                o.state,
                o.name,
                o.uid
            ),
        })
        .collect()
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
            "fael: src/b/x.rs outside chunk U1's scope, inside running chunk p/b's — coordinate: fael chunk note U2 \"<text>\", or go on; said once per chunk, chunk done lists every file\n"
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
}
