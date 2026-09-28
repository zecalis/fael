//! `[Fat]` (PLAN-fael-row-hygiene chunk 10): open rows the chunk-3 add-time
//! warnings would have flagged (no key, several topics, long text) — the
//! agent skipped the warning, so `doctor` repeats it. Split out of
//! `maintain.rs` next to `orphan.rs`/`merged.rs` so `open_row_notes` stays
//! under the 100-line function cap. The conditions live in core
//! (`fat_reasons`) — never duplicated here.
//!
//! PLAN-fael-durable-log chunk 4: rows born before self-heal (chunk 3) are
//! `legacy` — they collapse to one line (`N legacy rows — fael doctor --fat
//! --json for a one-time pass`) so a repo with a hundred old rows still gets
//! a skimmable `doctor`. `--fat` expands them for the one-time cleanup pass;
//! new fat rows always list individually.

use crate::core;

/// First ms of 2026-09-28 UTC: chunk 3 (self-heal on add) landed that day,
/// so anything born before it never got an auto-key or auto-supersede.
pub(super) const LEGACY_CUTOFF_MS: u64 = 1_790_553_600_000;

/// One fat row: `(full id, short-id → first fat reason, birth)`. The birth
/// feeds the legacy split (`None` = unknowable, counted as legacy); the full
/// id rides to `doctor --json` for the cleanup pass.
type FatRow = (String, String, Option<u64>);

/// The `[Fat]` doctor problem, if any open row is fat — kept here (not in
/// `maintain.rs`) so `open_row_notes` stays under the 100-line function cap.
/// Without `expand`, legacy rows collapse to a single line; `--fat` lists
/// every fat row for the one-time cleanup pass.
pub(super) fn problem(log: &core::Log, cfg: &core::Config, expand: bool) -> Option<core::Problem> {
    let fat = rows(log, cfg);
    if fat.is_empty() {
        return None;
    }
    if expand {
        return Some(fat_problem(&fat, None, true));
    }
    let (new, legacy): (Vec<&FatRow>, Vec<&FatRow>) =
        fat.iter().partition(|(_, _, birth)| !is_legacy(*birth));
    if legacy.is_empty() {
        return Some(fat_problem(&fat, None, false));
    }
    if new.is_empty() {
        return Some(legacy_problem(legacy.len()));
    }
    let listed: Vec<FatRow> = new.into_iter().cloned().collect();
    Some(fat_problem(&listed, Some(legacy.len()), false))
}

/// One `[Fat]` problem. `full` lists every fat row — the `--fat` cleanup pass
/// exists to enumerate them all, so it must not drop any; otherwise the detail
/// skims with up to 5 examples. The optional `legacy` suffix collapses
/// pre-self-heal rows when old and new rows share the output. `ids` carries
/// every row behind the line, so `--json` never hides one.
fn fat_problem(fat: &[FatRow], legacy: Option<usize>, full: bool) -> core::Problem {
    let shown: Vec<&str> = fat.iter().map(|(_, s, _)| s.as_str()).collect();
    let n = if full {
        shown.len()
    } else {
        shown.len().min(5)
    };
    let mut detail = format!(
        "{} open row(s) carry no key, several topics, or long text — split them so one can \
         be superseded alone (e.g. {})",
        fat.len(),
        shown[..n].join("; ")
    );
    if let Some(n) = legacy {
        detail.push_str(&format!(
            " · {n} legacy rows — fael doctor --fat --json for a one-time pass"
        ));
    }
    core::Problem::info(core::ProblemKind::Fat, detail)
        .with_ids(fat.iter().map(|(id, _, _)| id.clone()).collect())
}

/// The collapsed legacy line: no ids, just the count and the cleanup pass.
fn legacy_problem(n: usize) -> core::Problem {
    core::Problem::info(
        core::ProblemKind::Fat,
        format!("{n} legacy rows — fael doctor --fat --json for a one-time pass"),
    )
}

/// A row is legacy when its birth predates self-heal — or when its birth is
/// unknowable (non-ULID id, unparsable `ts`): only hand-written or imported
/// pre-ULID rows look like that, and those are old by definition.
fn is_legacy(birth: Option<u64>) -> bool {
    birth.is_none_or(|b| b < LEGACY_CUTOFF_MS)
}

/// The row's birth in unix ms: the ULID time first, the `ts` field as
/// fallback (same order as the `[Shipped]` check in `shipped.rs`).
fn birth_ms(row: &core::Row) -> Option<u64> {
    core::ulid_ms(&row.id).or_else(|| core::ts_ms(&row.ts).and_then(|t| u64::try_from(t).ok()))
}

/// `(full id, short-id → first fat reason, birth)` for every open row
/// `fat_reasons` flags — closed rows never count (`find` hides them by default).
fn rows(log: &core::Log, cfg: &core::Config) -> Vec<FatRow> {
    let w = core::abbrev(log);
    core::find(log, &core::Filter::default())
        .into_iter()
        .filter_map(|row| {
            let rs = core::fat_reasons(row, cfg);
            (!rs.is_empty()).then(|| {
                (
                    row.id.clone(),
                    format!("{} → {}", w.short(&row.id), rs[0]),
                    birth_ms(row),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{LEGACY_CUTOFF_MS, is_legacy, problem};

    fn log() -> crate::core::Log {
        let old = crate::core::ulid_at(LEGACY_CUTOFF_MS - 1);
        let new = crate::core::ulid();
        crate::core::Log {
            rows: vec![
                crate::core::Row {
                    id: old,
                    ts: "2026-01-01T00:00:00Z".into(),
                    by: "t-0000".into(),
                    kind: "decision".into(),
                    text: "legacy fat decision".into(),
                    files: vec!["src/a.rs".into()],
                    ..crate::core::Row::default()
                },
                crate::core::Row {
                    id: new.clone(),
                    ts: "2026-10-01T00:00:00Z".into(),
                    by: "t-0000".into(),
                    kind: "decision".into(),
                    text: "new fat decision".into(),
                    files: vec!["src/a.rs".into()],
                    ..crate::core::Row::default()
                },
            ],
            ..crate::core::Log::default()
        }
    }

    fn legacy_only() -> crate::core::Log {
        let mut l = log();
        l.rows.truncate(1);
        l
    }

    #[test]
    fn legacy_rows_collapse_to_one_line() {
        let p = problem(&legacy_only(), &crate::core::Config::default(), false).unwrap();
        assert_eq!(
            p.detail, "1 legacy rows — fael doctor --fat --json for a one-time pass",
            "{}",
            p.detail
        );
    }

    #[test]
    fn mixed_lists_new_and_collapses_legacy() {
        let p = problem(&log(), &crate::core::Config::default(), false).unwrap();
        assert!(
            p.detail
                .contains("1 legacy rows — fael doctor --fat --json"),
            "{}",
            p.detail
        );
        assert!(
            p.detail.contains("new fat decision") || p.detail.contains("decision has no --key"),
            "{}",
            p.detail
        );
        // the legacy id stays hidden until the cleanup pass
        let legacy_id = &log().rows[0].id;
        assert!(!p.detail.contains(&legacy_id[..8]), "{}", p.detail);
    }

    #[test]
    fn expand_flag_lists_every_fat_row() {
        let l = log();
        let p = problem(&l, &crate::core::Config::default(), true).unwrap();
        assert!(p.detail.contains("2 open row(s)"), "{}", p.detail);
        assert!(p.detail.contains(&l.rows[0].id[..8]), "{}", p.detail);
        assert!(p.detail.contains(&l.rows[1].id[..8]), "{}", p.detail);
    }

    /// `n` new (post-cutoff) fat decisions, each its own millisecond so their
    /// abbreviated ids differ.
    fn fat_log(n: usize) -> crate::core::Log {
        crate::core::Log {
            rows: (0..n)
                .map(|i| crate::core::Row {
                    id: crate::core::ulid_at(LEGACY_CUTOFF_MS + 1 + i as u64 * 1000),
                    ts: "2026-10-01T00:00:00Z".into(),
                    by: "t-0000".into(),
                    kind: "decision".into(),
                    text: "new fat decision".into(),
                    files: vec!["src/a.rs".into()],
                    ..crate::core::Row::default()
                })
                .collect(),
            ..crate::core::Log::default()
        }
    }

    #[test]
    fn expand_lists_every_row_past_the_five_example_cap() {
        let l = fat_log(7);
        let w = crate::core::abbrev(&l);
        let p = problem(&l, &crate::core::Config::default(), true).unwrap();
        assert!(p.detail.contains("7 open row(s)"), "{}", p.detail);
        for r in &l.rows {
            assert!(
                p.detail.contains(w.short(&r.id)),
                "missing {} in {}",
                r.id,
                p.detail
            );
        }
        // without `--fat` the same repo skims with five examples
        let g = problem(&l, &crate::core::Config::default(), false).unwrap();
        let named = l
            .rows
            .iter()
            .filter(|r| g.detail.contains(w.short(&r.id)))
            .count();
        assert_eq!(named, 5, "{}", g.detail);
    }

    #[test]
    fn ids_ride_on_the_fat_problem() {
        let l = fat_log(3);
        let p = problem(&l, &crate::core::Config::default(), true).unwrap();
        assert_eq!(p.ids.len(), l.rows.len());
        for r in &l.rows {
            assert!(p.ids.contains(&r.id), "missing {} in {:?}", r.id, p.ids);
        }
        // the collapsed legacy line carries no ids (the pass that expands it does)
        let legacy = legacy_only();
        let collapsed = problem(&legacy, &crate::core::Config::default(), false).unwrap();
        assert!(collapsed.ids.is_empty());
    }

    #[test]
    fn unknowable_birth_counts_as_legacy() {
        assert!(is_legacy(None));
        assert!(is_legacy(Some(LEGACY_CUTOFF_MS - 1)));
        assert!(!is_legacy(Some(LEGACY_CUTOFF_MS)));
    }
}
