//! `[Fat]` (PLAN-fael-row-hygiene chunk 10): open rows the chunk-3 add-time
//! warnings would have flagged (no key, several topics, long text) — the
//! agent skipped the warning, so `doctor` repeats it. Split out of
//! `maintain.rs` next to `orphan.rs`/`merged.rs` so `open_row_notes` stays
//! under the 100-line function cap. The conditions live in core
//! (`fat_reasons`) — never duplicated here.

use crate::core;

/// The `[Fat]` doctor problem, if any open row is fat — kept here (not in
/// `maintain.rs`) so `open_row_notes` stays under the 100-line function cap.
pub(super) fn problem(log: &core::Log, cfg: &core::Config) -> Option<core::Problem> {
    let fat = rows(log, cfg);
    if fat.is_empty() {
        return None;
    }
    Some(core::Problem {
        kind: core::ProblemKind::Fat,
        severity: core::Severity::Info,
        fixable: false,
        file: None,
        detail: format!(
            "{} open row(s) carry no key, several topics, or long text — split them so one can \
             be superseded alone (e.g. {})",
            fat.len(),
            fat[..fat.len().min(5)].join("; ")
        ),
    })
}

/// `short-id → first fat reason` for every open row `fat_reasons` flags —
/// closed rows never count (`find` hides them by default).
fn rows(log: &core::Log, cfg: &core::Config) -> Vec<String> {
    let w = core::abbrev(log);
    core::find(log, &core::Filter::default())
        .into_iter()
        .filter_map(|row| {
            let rs = core::fat_reasons(row, cfg);
            (!rs.is_empty()).then(|| format!("{} → {}", core::short_id(&row.id, w), rs[0]))
        })
        .collect()
}
