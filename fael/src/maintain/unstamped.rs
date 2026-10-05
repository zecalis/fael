//! `[Unstamped]`: open rows with no `fh` that name a real file the edit push
//! could compare (≤ `PUSH_MAX_BYTES`) — written before file hashes existed,
//! or from a path that was not there yet. Push has no stamp to compare, so
//! these rows never earn a changed/unchanged verdict and get the generic
//! hint for as long as they stay open. A row checked against the file and
//! still true gets `fael bump <id>` (a bare bump restamps from disk). A fact
//! from `metadata()` (no reads), never a verdict; over-cap files are
//! `[NoVerdict]`'s, a missing file is `[PartGone]`'s, an anchor or glob has no
//! file, and closed/superseded rows are not checked.

use crate::core;
use crate::hook::{PUSH_MAX_BYTES, is_anchor};
use std::path::Path;

/// The first named file of `row` that a stamp could cover: a regular file
/// the push would hash.
fn stampable<'a>(root: &Path, row: &'a core::Row) -> Option<&'a str> {
    row.files
        .iter()
        .filter(|f| !is_anchor(f) && !core::is_glob(f))
        .find(|f| {
            std::fs::metadata(root.join(f))
                .is_ok_and(|md| md.is_file() && md.len() <= PUSH_MAX_BYTES)
        })
        .map(String::as_str)
}

/// The `[Unstamped]` note, in log order.
pub(super) fn problem(log: &core::Log, root: &Path) -> Option<core::Problem> {
    let w = core::abbrev(log);
    let hits: Vec<(&core::Row, &str)> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter(|r| r.file_hashes().is_none())
        .filter_map(|r| stampable(root, r).map(|f| (r, f)))
        .collect();
    if hits.is_empty() {
        return None;
    }
    let eg: Vec<String> = hits
        .iter()
        .take(5)
        .map(|(r, f)| format!("{} → {f}", w.short(&r.id)))
        .collect();
    Some(
        core::Problem::info(
            core::ProblemKind::Unstamped,
            format!(
                "{} open row(s) have no file stamp — push cannot say whether their files changed \
                 since the row was written; check each against the file: still true → `fael bump \
                 <id>` stamps it (e.g. {})",
                hits.len(),
                eg.join("; ")
            ),
        )
        .with_ids(hits.iter().map(|(r, _)| r.id.clone()).collect()),
    )
}
