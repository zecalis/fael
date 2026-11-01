//! `fael chunk done`: the notes this session filed during the run that are still open.
//! A checkpoint written mid-run ("gate blocked by main's typecheck error") outlives the fix
//! unless someone closes it, and the next reader takes it as current. The done lists them
//! with the three verdicts ready; the agent judges, done never closes (issue 01M6BMFK).

use super::changed::own_row;
use crate::core;

/// At most this many; a long list would be a wall nobody reads.
const MAX: usize = 3;

/// One line per open note of the caller's session written at or after `since` (the run's
/// start, ms). Empty without a session, a start time, or such a note.
pub(crate) fn text(r: &crate::Repo, since: Option<i64>) -> String {
    let (Some(since), Some(session)) = (since, crate::session::writer_session(&r.root)) else {
        return String::new();
    };
    let log = crate::read(r);
    let rows: Vec<&core::Row> = log
        .rows
        .iter()
        .filter(|w| w.kind == "note" && own_row(w, &session))
        .filter(|w| core::ts_ms(&w.ts).is_some_and(|t| t >= since))
        .filter(|w| super::held::active(&log)(&w.id))
        .collect();
    let ab = core::abbrev(&log);
    let mut out = String::new();
    for w in rows.iter().take(MAX) {
        let id = ab.short(&w.id);
        let title = w
            .title
            .as_deref()
            .map(|t| format!(" \"{t}\""))
            .unwrap_or_default();
        out.push_str(&format!(
            "fael: note {id}{title} — you filed it this run and it is still open · fixed since? fael close {id} \"<why>\" · changed? re-file with --supersedes {id} · still true? leave it\n"
        ));
    }
    if rows.len() > MAX {
        out.push_str(&format!(
            "+{} more of your notes: fael find --kind note\n",
            rows.len() - MAX
        ));
    }
    out
}
