//! Model/cost attribution for `fael stats` (plan fael-session-friction,
//! chunk stats-unknown): what share of this clone's crew.db runs carry the
//! client-reported client+model and a cost above zero, and where the money
//! went. Pure over the loaded run rows: NULL stays NULL, percentages round
//! to one decimal, empty reads zero (never NaN). Same unknown-% math as the
//! board's `Models` (`fael-crew`), recomputed here so `fael` never links
//! `fael-crew` — the two cannot drift on the definition, only on the rows
//! each side reads (stats: this clone's db; board: every repo it merges).

use serde::Serialize;
use std::collections::BTreeMap;

/// One run row for the attribution join: everything NULL-able, like the table.
#[derive(Debug, Clone)]
pub struct ModelRun {
    pub client: Option<String>,
    pub model: Option<String>,
    pub cost: Option<f64>,
}

/// Runs and summed USD for one `client/model` group (a missing part reads
/// `unknown`); cost ≤ 0 adds 0 to the sum but counts as unknown for the
/// metric — it doesn't demonstrate attribution.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ModelRow {
    pub runs: usize,
    pub cost: f64,
    pub unknown_cost_pct: f64,
    /// Runs in this group lacking cost (the merge key; never serialized).
    #[serde(skip)]
    pub missing: usize,
}

/// The `models` key of `fael stats --json`: totals with unknown-% beside
/// every cost number, per-model sums. Old NULL rows stay NULL — the
/// unknown-% is the point, not a gap to fill.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Models {
    /// Every run row read.
    pub total: usize,
    /// Runs with non-null client AND model.
    pub with_client_model: usize,
    /// Runs with cost > 0.
    pub with_cost: usize,
    /// 0-100, one decimal: runs lacking client+model / total (0 when empty).
    pub unknown_model_pct: f64,
    /// 0-100, one decimal: runs without cost > 0 / total (0 when empty).
    pub unknown_cost_pct: f64,
    /// `client/model` → runs and summed USD.
    pub by_model: BTreeMap<String, ModelRow>,
}

/// Unknown-%: 0-100 with one decimal (0 when empty, never NaN).
fn pct(missing: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (missing as f64 * 1000.0 / total as f64).round() / 10.0
}

/// Aggregate run rows into the `models` read. A zero or negative cost reads
/// as unknown for the metric; a NaN or infinite cost is dropped, never stored
/// (the write side already filters those, this just never trusts the table).
pub fn models(runs: &[ModelRun]) -> Models {
    let total = runs.len();
    let with_client_model = runs
        .iter()
        .filter(|r| r.client.is_some() && r.model.is_some())
        .count();
    let with_cost = runs
        .iter()
        .filter(|r| r.cost.is_some_and(|c| c > 0.0))
        .count();
    let mut by_model: BTreeMap<String, ModelRow> = BTreeMap::new();
    for r in runs {
        let key = format!(
            "{}/{}",
            r.client.as_deref().unwrap_or("unknown"),
            r.model.as_deref().unwrap_or("unknown")
        );
        let e = by_model.entry(key).or_default();
        e.runs += 1;
        if r.cost.is_some_and(|c| c > 0.0) {
            e.cost += r.cost.unwrap_or(0.0);
        } else {
            e.missing += 1;
        }
    }
    for e in by_model.values_mut() {
        e.unknown_cost_pct = pct(e.missing, e.runs);
    }
    Models {
        total,
        with_client_model,
        with_cost,
        unknown_model_pct: pct(total - with_client_model, total),
        unknown_cost_pct: pct(total - with_cost, total),
        by_model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(client: Option<&str>, model: Option<&str>, cost: Option<f64>) -> ModelRun {
        ModelRun {
            client: client.map(String::from),
            model: model.map(String::from),
            cost,
        }
    }

    #[test]
    fn mixed_null_and_complete_renders_unknown_pct_exactly() {
        let m = models(&[
            run(Some("claude"), Some("opus"), Some(2.17)),
            run(Some("claude"), Some("haiku"), None),
            run(None, None, None),
        ]);
        assert_eq!((m.total, m.with_client_model, m.with_cost), (3, 2, 1));
        assert_eq!((m.unknown_model_pct, m.unknown_cost_pct), (33.3, 66.7));
        let opus = &m.by_model["claude/opus"];
        assert_eq!((opus.runs, opus.unknown_cost_pct), (1, 0.0));
        assert!((opus.cost - 2.17).abs() < 1e-9);
        let bare = &m.by_model["unknown/unknown"];
        assert_eq!((bare.runs, bare.unknown_cost_pct), (1, 100.0));
    }

    #[test]
    fn empty_reads_zero_not_nan() {
        let m = models(&[]);
        assert_eq!(
            (m.total, m.unknown_model_pct, m.unknown_cost_pct),
            (0, 0.0, 0.0)
        );
        assert!(m.by_model.is_empty());
    }

    #[test]
    fn zero_and_non_finite_cost_read_as_unknown() {
        let m = models(&[
            run(Some("opencode"), Some("opencode-go/m"), Some(0.0)),
            run(Some("opencode"), Some("opencode-go/m"), Some(0.01)),
            run(Some("opencode"), Some("opencode-go/m"), Some(f64::NAN)),
        ]);
        assert_eq!((m.with_cost, m.unknown_cost_pct), (1, 66.7));
        assert_eq!(
            m.by_model["opencode/opencode-go/m"].unknown_cost_pct,
            66.7
        );
    }
}
