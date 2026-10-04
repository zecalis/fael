//! `[NoVerdict]`: open rows that name a real file bigger than the edit push
//! compares (`PUSH_MAX_BYTES`). `fael add` stamps files up to 16 MiB, but the
//! push only hashes up to the cap, so such a row gets the generic hint for as
//! long as it stays open — silently, until this note. A fact from `metadata()`
//! (no reads), never a verdict; a missing file is `[PartGone]`'s, an anchor or
//! glob has no file, and closed/superseded rows are not checked.

use crate::core;
use crate::hook::{PUSH_MAX_BYTES, is_anchor};
use std::path::Path;

const MIB: f64 = 1024.0 * 1024.0;

/// The first named file of `row` that is a regular file over the push cap,
/// with its size in bytes.
fn over_cap(root: &Path, row: &core::Row) -> Option<(String, u64)> {
    row.files
        .iter()
        .filter(|f| !is_anchor(f) && !core::is_glob(f))
        .find_map(|f| {
            let md = std::fs::metadata(root.join(f)).ok()?;
            (md.is_file() && md.len() > PUSH_MAX_BYTES).then(|| (f.clone(), md.len()))
        })
}

/// The `[NoVerdict]` note, in log order.
pub(super) fn problem(log: &core::Log, root: &Path) -> Option<core::Problem> {
    let w = core::abbrev(log);
    let hits: Vec<(&core::Row, String, u64)> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter_map(|r| over_cap(root, r).map(|(f, n)| (r, f, n)))
        .collect();
    if hits.is_empty() {
        return None;
    }
    let eg: Vec<String> = hits
        .iter()
        .take(5)
        .map(|(r, f, n)| format!("{} → {f} ({:.1} MiB)", w.short(&r.id), *n as f64 / MIB))
        .collect();
    Some(
        core::Problem::info(
            core::ProblemKind::NoVerdict,
            format!(
                "{} open row(s) name a file over the {} MiB push cap — push never says whether \
                 these files changed since the row was written; the generic hint stands (e.g. {})",
                hits.len(),
                PUSH_MAX_BYTES / (1024 * 1024),
                eg.join("; ")
            ),
        )
        .with_ids(hits.iter().map(|(r, _, _)| r.id.clone()).collect()),
    )
}
