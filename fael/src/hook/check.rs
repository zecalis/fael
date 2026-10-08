//! The gone-check ask (PLAN-fael-experience-loop chunk 3): an edit of a file
//! that a closed issue sits on, whose close text names a backticked path with
//! nothing behind it any more, says so once per issue and session. fael does
//! not judge the check — it only finds the pointer dead; the agent restores
//! it, or files what is unguarded now.

use super::changed::Ask;
use super::say::{Kind, Line};
use crate::core;

/// Paths named in the line; the rest are one `fael find <id>` away.
const NAMED: usize = 2;

/// The edit-time asks beyond the edit hint: the consolidate ask, then the
/// gone-check ask. A read (`edit` false) gets neither.
pub(crate) fn asks(
    ask: &Ask,
    t0: &[(&core::Row, usize)],
    said: &[&core::Row],
    edit: bool,
) -> Vec<Line> {
    let merge = super::merge::merge_line(ask, t0, said);
    merge.into_iter().chain(check_line(ask, edit)).collect()
}

/// The first closed issue on an edited file whose close names a gone path, as
/// one `Check` line. `edit` is false off an edit: a read gets no line.
pub(crate) fn check_line(ask: &Ask, edit: bool) -> Option<Line> {
    if !edit {
        return None;
    }
    let ab = core::abbrev(ask.log);
    ask.log.rows.iter().find_map(|r| {
        let file = ask.files.iter().find(|f| r.files.contains(f))?;
        if r.kind != "issue" {
            return None;
        }
        let gone = core::stale_close_refs(ask.root, ask.log, r, ask.al);
        let named: Vec<String> = gone.iter().take(NAMED).map(|p| format!("`{p}`")).collect();
        let id = ab.short(&r.id);
        (!named.is_empty()).then(|| Line {
            kind: Kind::Check { id: r.id.clone() },
            text: format!(
                "fael: {id} was closed pointing at {}, now gone — restore it, or file what is unguarded: `fael add issue \"<what regressed>\" --files {file} --supersedes {id}`\n",
                named.join(" · ")
            ),
        })
    })
}
