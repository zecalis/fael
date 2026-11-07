//! Brief drift of a running chunk (plan fael-chunk-drift): the query is core's, so the
//! hook and `chunk` output agree; this is its door on a `Store`.

use super::store::{Store, err};
pub use fael_core::work::Drift;

/// A running chunk's live run, as `chunk status` and the board's `drift N` read it.
#[derive(Debug, Clone)]
pub struct Live {
    pub uid: String,
    pub started: String,
    pub base: Option<String>,
    pub worktree: Option<String>,
    pub pr: Option<i64>,
    /// the files the run reported (`touched`), else the declared scope
    pub files: Vec<String>,
}

/// A chunk closed (done or replaced) at `at`: main-drift's "closed since start".
#[derive(Debug, Clone)]
pub struct Shut {
    pub uid: String,
    pub label: Option<String>,
    pub title: String,
    pub scope: Vec<String>,
    pub at: String,
}

fn lines(s: Option<String>) -> Vec<String> {
    s.unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

impl Store {
    /// What the owner edited (brief, scope, title) since `uid`'s latest start.
    pub fn drift(&self, uid: &str) -> Option<Drift> {
        fael_core::work::drift_in(&self.conn, uid)
    }

    /// The live run of every running chunk (or of `only`), newest run per chunk. DB only.
    pub fn live_runs(&self, only: Option<&str>) -> Result<Vec<Live>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT c.uid, r.started, r.base, r.worktree, r.pr, r.touched, c.scope
                 FROM chunk c JOIN run r ON r.chunk = c.id
                 WHERE c.state = 'running' AND r.ended IS NULL
                   AND r.id = (SELECT MAX(id) FROM run WHERE chunk = c.id)
                   AND (?1 IS NULL OR c.uid = ?1)",
            )
            .map_err(err)?;
        q.query_map([only], |r| {
            let touched = lines(r.get(5)?);
            Ok(Live {
                uid: r.get(0)?,
                started: r.get(1)?,
                base: r.get(2)?,
                worktree: r.get(3)?,
                pr: r.get(4)?,
                files: if touched.is_empty() {
                    lines(r.get(6)?)
                } else {
                    touched
                },
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }

    /// Chunks that went done or replaced after `since`, with when. DB only.
    pub fn shut_since(&self, since: &str) -> Result<Vec<Shut>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT uid, label, title, scope, at FROM (
                   SELECT c.uid, c.label, c.title, c.scope,
                     (SELECT MAX(e.at) FROM event e WHERE e.chunk_uid = c.uid AND e.to_state = c.state) AS at
                   FROM chunk c WHERE c.state IN ('done', 'replaced'))
                 WHERE at > ?1",
            )
            .map_err(err)?;
        q.query_map([since], |r| {
            Ok(Shut {
                uid: r.get(0)?,
                label: r.get(1)?,
                title: r.get(2)?,
                scope: lines(r.get(3)?),
                at: r.get(4)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }

    /// `drift` as the line `chunk note|done` print; empty without drift.
    pub fn drift_line(&self, uid: &str) -> String {
        self.drift(uid).map(|d| d.text()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::super::chunk::tests::{NOW, add, start};
    use super::super::{Fields, Store};

    #[test]
    fn a_brief_edit_after_start_drifts_one_before_does_not() {
        let mut s = Store::open_in_memory().unwrap();
        let u = add(&mut s, "a", Some("a"));
        let edit = |s: &mut Store, b: &str| {
            let f = Fields {
                brief: Some(b.into()),
                ..Default::default()
            };
            s.edit(&u, &f, NOW).unwrap();
        };
        edit(&mut s, "before");
        start(&mut s, &u, "/w").unwrap();
        assert_eq!(s.drift(&u), None, "edited before the start");
        edit(&mut s, "after");
        let d = s.drift(&u).unwrap();
        assert_eq!(d.fields, ["brief"]);
        start(&mut s, &u, "/w").unwrap();
        assert_eq!(s.drift(&u), None, "a new start resets it");
    }
}
