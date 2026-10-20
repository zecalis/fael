//! The edit-time scope overlap line (issue 01M5D9GB): a worktree holding a
//! live chunk run edits a file outside that chunk's declared scope (its
//! pair's with it) that sits under another unfinished chunk's scope — two
//! agents on one file who may not know it. Said once per file and session,
//! with the ready `fael chunk note`. An unowned file outside scope says
//! nothing (a plain git merge); `chunk done` still lists it. Inform only: no
//! edit is blocked. One read-only SQLite read, no git spawn; `docs/` and
//! `.fael/shared` paths are nobody's, as in `Store::outside`.

use super::protocol::Ctx;
use super::say::{Kind, Line};
use crate::core::work;
use std::path::Path;

/// One `Overlap` line per edited file that fits the rule. Empty with no
/// crew.db, no live run, no declared scope or no overlapping chunk.
pub(crate) fn lines(c: &Ctx, files: &[String]) -> Vec<Line> {
    let db = work::db_for(c.repo.journal.as_deref(), &c.repo.fael);
    at(&db, &c.repo.root, files)
}

fn at(db: &Path, root: &Path, files: &[String]) -> Vec<Line> {
    if files.is_empty() || !db.exists() {
        return vec![];
    }
    let shared = work::shared(root).unwrap_or_default();
    match work::scopes(db, &root.to_string_lossy(), &shared) {
        Some(s) => about(&s, &shared, files),
        None => vec![],
    }
}

fn about(s: &work::Scopes, shared: &[String], files: &[String]) -> Vec<Line> {
    files
        .iter()
        .filter_map(|f| {
            let o = s.overlap(f, shared)?;
            Some(Line {
                kind: Kind::Overlap { file: f.clone() },
                text: format!(
                    "fael: {f} is outside chunk {}'s scope and inside open chunk {}'s — coordinate: fael chunk note {} \"<text>\", or go on\n",
                    s.uid, o.name, o.uid
                ),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::say::Outbox;
    use crate::hook::state::lock_seen;

    fn scopes() -> work::Scopes {
        work::Scopes {
            uid: "U1".into(),
            declared: vec!["src/a/".into()],
            others: vec![work::Other {
                uid: "U2".into(),
                name: "p/b".into(),
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
            "fael: src/b/x.rs is outside chunk U1's scope and inside open chunk p/b's — coordinate: fael chunk note U2 \"<text>\", or go on\n"
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
        assert!(at(&none.join("crew.db"), &none, &["src/b/x.rs".into()]).is_empty());
    }

    #[test]
    fn a_second_edit_of_the_same_file_is_silent() {
        let seen = crate::hook::say_contract::seen("s.seen");
        let say = || {
            let mut out = Outbox::open(lock_seen(&seen));
            one("src/b/x.rs").into_iter().for_each(|l| out.say(l));
            out.reply().context().map(String::from)
        };
        assert!(say().is_some());
        assert_eq!(say(), None);
    }
}
