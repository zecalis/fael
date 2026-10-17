//! The runs `chunk done` ended (decision 01M5528X, row retire): what a note can be
//! proven to belong to. A fact read from the db — never a guess from the brief.

use super::store::{Store, err};

/// One run that `fael chunk done` ended on a chunk now in `review` or `done`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndedRun {
    pub plan: String,
    pub session: Option<String>,
    pub branch: Option<String>,
    /// when the run ended (rfc3339)
    pub ended: String,
}

impl Store {
    pub fn ended_runs(&self) -> Result<Vec<EndedRun>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT p.name, r.session, r.branch, r.ended FROM run r
                 JOIN chunk c ON c.id = r.chunk JOIN plan p ON p.id = c.plan
                 WHERE r.ended IS NOT NULL AND c.state IN ('review','done')",
            )
            .map_err(err)?;
        q.query_map([], |r| {
            Ok(EndedRun {
                plan: r.get(0)?,
                session: r.get(1)?,
                branch: r.get(2)?,
                ended: r.get(3)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }
}
