//! The scopes an edit-time overlap line compares (issue 01M5D9GB, f5): what the
//! chunk a worktree holds declared (its pair's with it) and what every other
//! unfinished chunk declared — plus each live run's touched files beside its
//! declared scope. Same prefix rule as the board (`under`) and the same read
//! as `Store::outside` in fael-crew, kept here read-only so core
//! never depends on crew.

use super::db::readonly;
use crate::{under, within};
use std::path::Path;

/// Another unfinished chunk: its declared scope and its live run's touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Other {
    pub uid: String,
    /// `plan/label` (the uid when it has no label), as `chunk done` names it.
    pub name: String,
    /// The chunk's `state` as stored (`running`, `open`, `draft`, `review`...).
    pub state: String,
    pub scope: Vec<String>,
    /// The live run's branch, if it has one.
    pub branch: Option<String>,
    /// The live run's client, if reported (e.g. `claude`, `opencode`).
    pub client: Option<String>,
    /// `run.touch_msg`: what the partner said it is doing, if reported.
    pub doing: Option<String>,
    /// The live run's `started`, for the `since` the edit line shows.
    pub since: Option<String>,
    /// The live run's `last_seen`, for the `live` vs `stalled Nm` label.
    pub last_seen: Option<String>,
    /// The live run's `touched` (`chunk touch` so far), less `shared`.
    pub touched: Vec<String>,
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
    let others = others(&c, worktree, session, shared)?;
    Some(Scopes {
        uid,
        declared,
        others,
    })
}

/// The unfinished chunks around the held team, with each live run's touched
/// files beside its declared scope. Falls back to scope-only when the db
/// predates `run.touched`/`touch_msg` (added as columns, no epoch bump).
fn others(
    c: &rusqlite::Connection,
    worktree: &str,
    session: &str,
    shared: &[String],
) -> Option<Vec<Other>> {
    type Row = (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let live = "r.id = (SELECT x.id FROM run x WHERE x.chunk = c.id
        AND x.ended IS NULL ORDER BY x.started DESC LIMIT 1)";
    let rest = format!(
        "{TEAM} SELECT c.uid, p.name || '/' || COALESCE(c.label, c.uid), c.state, c.scope,
          r.branch, r.client, r.touch_msg, r.touched, r.started, r.last_seen
         FROM chunk c JOIN plan p ON p.id = c.plan
         LEFT JOIN run r ON {live}
         WHERE c.state NOT IN ('done', 'replaced', 'dropped')
         AND c.id NOT IN (SELECT id FROM team)
         AND (c.scope IS NOT NULL OR r.touched IS NOT NULL) ORDER BY c.seq"
    );
    if let Ok(rows) = c.prepare(&rest).and_then(|mut q| {
        q.query_map([worktree, session], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        })
        .and_then(|rows| rows.collect::<Result<Vec<Row>, _>>())
    }) {
        return Some(rows.into_iter().map(|r| to_other(r, shared)).collect());
    }
    fallback(c, worktree, session, shared)
}

type OtherRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn to_other(row: OtherRow, shared: &[String]) -> Other {
    let (uid, name, state, s, branch, client, doing, touched, since, last_seen) = row;
    Other {
        uid,
        name,
        state,
        scope: lines(s)
            .into_iter()
            .filter(|p| !shared.iter().any(|s| under(p, s)))
            .collect(),
        branch: branch.filter(|b| !b.is_empty()),
        client: client.filter(|c| !c.is_empty()),
        doing: doing.filter(|d| !d.is_empty()),
        since: since.filter(|s| !s.is_empty()),
        last_seen: last_seen.filter(|s| !s.is_empty()),
        touched: lines(touched)
            .into_iter()
            .filter(|p| !shared.iter().any(|s| under(p, s)))
            .collect(),
    }
}

fn fallback(
    c: &rusqlite::Connection,
    worktree: &str,
    session: &str,
    shared: &[String],
) -> Option<Vec<Other>> {
    let rest = format!(
        "{TEAM} SELECT c.uid, p.name || '/' || COALESCE(c.label, c.uid), c.state, c.scope
         FROM chunk c JOIN plan p ON p.id = c.plan
         WHERE c.scope IS NOT NULL AND c.state NOT IN ('done', 'replaced', 'dropped')
         AND c.id NOT IN (SELECT id FROM team) ORDER BY c.seq"
    );
    c.prepare(&rest)
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
            branch: None,
            client: None,
            doing: None,
            since: None,
            last_seen: None,
            touched: Vec::new(),
        })
        .collect::<Vec<_>>()
        .into()
}

impl Scopes {
    /// The unfinished chunk whose scope covers `file` when `file` is outside
    /// the held chunk's own, or whose live run touched `file` even inside it.
    /// Touched wins: a live file beats a declared dir. A `docs/` or `shared`
    /// path is nobody's; no declared scope = nothing to compare. An unowned
    /// path is `None`.
    pub fn overlap(&self, file: &str, shared: &[String]) -> Option<&Other> {
        self.touched_overlap(file, shared)
            .or_else(|| self.scope_overlap(file, shared))
    }

    /// The unfinished chunk whose declared scope covers `file` when `file` is
    /// outside the held chunk's own. The old rule, kept for its own tests.
    pub fn scope_overlap(&self, file: &str, shared: &[String]) -> Option<&Other> {
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

    /// The unfinished chunk whose live run touched `file`, even inside the
    /// held chunk's own scope. A prefix either way counts (`src/` covers
    /// `src/a.rs`), the board's rule; `docs/` and `shared` stay nobody's.
    pub fn touched_overlap(&self, file: &str, shared: &[String]) -> Option<&Other> {
        let nobody = file.starts_with("docs/") || shared.iter().any(|s| under(file, s));
        (!self.declared.is_empty() && !nobody)
            .then(|| {
                self.others.iter().find(|o| {
                    !o.touched.is_empty()
                        && o.touched.iter().any(|t| under(file, t) || under(t, file))
                })
            })
            .flatten()
    }
}

#[cfg(test)]
#[path = "scopes_tests.rs"]
mod tests;
