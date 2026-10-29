//! The scopes an edit-time overlap line compares (issue 01M5D9GB): what the
//! chunk a worktree holds declared (its pair's with it) and what every other
//! unfinished chunk declared. Same prefix rule as the board (`under`) and the
//! same read as `Store::outside` in fael-crew, kept here read-only so core
//! never depends on crew.

use super::db::readonly;
use crate::{under, within};
use std::path::Path;

/// Another unfinished chunk with a declared scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Other {
    pub uid: String,
    /// `plan/label` (the uid when it has no label), as `chunk done` names it.
    pub name: String,
    /// The chunk's `state` as stored (`running`, `open`, `draft`, `review`...).
    pub state: String,
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scopes {
    /// The held chunk (the first when a pair is held).
    pub uid: String,
    /// The held chunk's scope and its pair partners'.
    pub declared: Vec<String>,
    pub others: Vec<Other>,
}

const TEAM: &str = "WITH mine(id) AS (SELECT c.id FROM run r JOIN chunk c ON c.id = r.chunk
      WHERE (r.worktree = ?1 OR (?2 <> '' AND r.session = ?2)) AND r.ended IS NULL),
    team(id) AS (SELECT id FROM mine
      UNION SELECT e.dst FROM edge e JOIN mine ON mine.id = e.src WHERE e.kind = 'pair'
      UNION SELECT e.src FROM edge e JOIN mine ON mine.id = e.dst WHERE e.kind = 'pair')";

fn lines(s: Option<String>) -> Vec<String> {
    s.unwrap_or_default().lines().map(String::from).collect()
}

/// The worktree's held chunk and the scopes around it, with `shared` paths
/// left out of the others'. `None` when no db, no tables or no live run.
pub fn scopes(db: &Path, worktree: &str, session: &str, shared: &[String]) -> Option<Scopes> {
    let c = readonly(db)?;
    let mine = format!(
        "{TEAM} SELECT c.uid, c.scope, c.id IN (SELECT id FROM mine) FROM chunk c
         WHERE c.id IN (SELECT id FROM team) ORDER BY c.seq"
    );
    let rows = c
        .prepare(&mine)
        .ok()?
        .query_map([worktree, session], |r| {
            Ok((r.get::<_, String>(0)?, r.get(1)?, r.get::<_, bool>(2)?))
        })
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let uid = rows.iter().find(|r| r.2)?.0.clone();
    let declared = rows.into_iter().flat_map(|r| lines(r.1)).collect();
    let rest = format!(
        "{TEAM} SELECT c.uid, p.name || '/' || COALESCE(c.label, c.uid), c.state, c.scope
         FROM chunk c JOIN plan p ON p.id = c.plan
         WHERE c.scope IS NOT NULL AND c.state NOT IN ('done', 'replaced', 'dropped')
         AND c.id NOT IN (SELECT id FROM team) ORDER BY c.seq"
    );
    let others = c
        .prepare(&rest)
        .ok()?
        .query_map([worktree, session], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?
        .into_iter()
        .map(|(uid, name, state, s)| Other {
            uid,
            name,
            state,
            scope: lines(s)
                .into_iter()
                .filter(|p| !shared.iter().any(|s| under(p, s)))
                .collect(),
        })
        .collect();
    Some(Scopes {
        uid,
        declared,
        others,
    })
}

impl Scopes {
    /// The unfinished chunk whose scope covers `file` when `file` is outside
    /// the held chunk's own. A `docs/` or `shared` path is nobody's; no
    /// declared scope = nothing to compare. An unowned path is `None`.
    pub fn overlap(&self, file: &str, shared: &[String]) -> Option<&Other> {
        let nobody = file.starts_with("docs/") || shared.iter().any(|s| under(file, s));
        let mine = self.declared.iter().any(|d| within(file, d));
        (!self.declared.is_empty() && !nobody && !mine)
            .then(|| {
                self.others
                    .iter()
                    .find(|o| o.scope.iter().any(|d| under(file, d)))
            })
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(rows: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("fael-scopes-{}.db", crate::ulid()));
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(&format!(
            "CREATE TABLE plan(id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT, label TEXT, plan INTEGER,
               seq INTEGER, scope TEXT, state TEXT);
             CREATE TABLE run(chunk INTEGER, worktree TEXT, ended TEXT, session TEXT);
             CREATE TABLE edge(src INTEGER, dst INTEGER, kind TEXT);
             INSERT INTO plan VALUES (1, 'p');
             {rows}"
        ))
        .unwrap();
        p
    }

    #[test]
    fn reads_held_scope_and_other_unfinished_scopes() {
        let p = db(
            "INSERT INTO chunk VALUES (1,'U1','a',1,1,'src/a/','running'),
              (2,'U2','b',1,2,'src/b/\nshared/x','ready'),
              (3,'U3','c',1,3,'src/c/','done'),
              (4,'U4',NULL,1,4,'src/d/','ready'),
              (5,'U5','e',1,5,'src/e/','review');
             INSERT INTO run VALUES (1,'/wt',NULL,'S'),(3,'/wt','2026-01-01',NULL);
             INSERT INTO edge VALUES (1,5,'pair');",
        );
        let s = scopes(&p, "/wt", "", &["shared/".into()]).unwrap();
        assert_eq!((s.uid.as_str(), s.declared.len()), ("U1", 2));
        let names: Vec<&str> = s.others.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(
            names,
            ["p/b", "p/U4"],
            "done, own and pair partner left out"
        );
        assert_eq!(s.others[0].scope, ["src/b/"], "shared path dropped");
        assert_eq!(s.others[0].state, "ready", "state as stored");
        assert!(scopes(&p, "/other", "", &[]).is_none());
        assert!(scopes(&p, "/other", "T", &[]).is_none(), "another session");
        assert_eq!(
            scopes(&p, "/other", "S", &[]).map(|s| s.uid),
            Some("U1".into()),
            "same session, other worktree"
        );
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn overlap_names_only_an_open_owner_outside_my_scope() {
        let s = Scopes {
            uid: "U1".into(),
            declared: vec!["src/a/".into()],
            others: vec![Other {
                uid: "U2".into(),
                name: "p/b".into(),
                state: "open".into(),
                scope: vec!["src/b/".into()],
            }],
        };
        assert_eq!(
            s.overlap("src/b/x.rs", &[]).map(|o| o.uid.as_str()),
            Some("U2")
        );
        assert!(s.overlap("src/a/x.rs", &[]).is_none(), "in scope");
        assert!(s.overlap("src/z.rs", &[]).is_none(), "unowned");
        assert!(
            s.overlap("src/b/x.rs", &["src/b/".into()]).is_none(),
            "shared"
        );
        let none = Scopes {
            declared: vec![],
            ..s
        };
        assert!(
            none.overlap("src/b/x.rs", &[]).is_none(),
            "no declared scope"
        );
    }
}
