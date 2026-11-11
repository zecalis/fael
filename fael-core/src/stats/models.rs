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

/// A size hint is not a reported model: `run.model` is set at start from
/// `--model`, else `approved_model`, else `model_hint`, and the hooks only
/// overwrite it with the transcript-reported model for live runs they match.
/// A bare size word (`S`/`M`/`L`, `small`/`medium`/`large` in any case) therefore
/// means no client ever reported a model — it must not count as known.
/// Fixed list, shared with the board (`fael-crew` calls this fn so the two
/// cannot drift on the definition, only on the rows each side reads).
pub fn is_hint(model: &str) -> bool {
    matches!(
        model.trim().to_ascii_lowercase().as_str(),
        "s" | "m" | "l" | "small" | "medium" | "large"
    )
}

/// The `models` key of `fael stats --json`: totals with unknown-% beside
/// every cost number, per-model sums. Old NULL rows stay NULL — the
/// unknown-% is the point, not a gap to fill. A size hint (`is_hint`) is not
/// a reported model: `with_client_model` needs client AND a non-hint model,
/// `hint_only` counts every hint-model row (any client) under `hint only`
/// instead of under a `client/model` group, and `none` is the remainder
/// (model NULL, or a real model with no client) — so
/// `reported + hint_only + none == total` and
/// `unknown_model_pct == (hint_only + none) / total`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Models {
    /// Every run row read.
    pub total: usize,
    /// Runs with non-null client AND a non-hint model (the reported share).
    pub with_client_model: usize,
    /// Runs whose model is a size hint (`is_hint`), any client — counted
    /// under `hint only`, never under a `client/model` group.
    pub hint_only: usize,
    /// The rest: model NULL, or a non-hint model with no client.
    pub none: usize,
    /// Runs with cost > 0.
    pub with_cost: usize,
    /// 0-100, one decimal: runs lacking client+reported-model / total (0 when empty).
    pub unknown_model_pct: f64,
    /// 0-100, one decimal: runs without cost > 0 / total (0 when empty).
    pub unknown_cost_pct: f64,
    /// `client/model` → runs and summed USD, hint-model rows excluded.
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
/// A hint-model row (`is_hint`) counts under `hint_only`, never under a
/// `client/model` group and never as reported — the unknown-model share
/// counts it as unknown. Stored rows are left alone (no backfill).
pub fn models(runs: &[ModelRun]) -> Models {
    let total = runs.len();
    let reported =
        |r: &&ModelRun| r.client.is_some() && r.model.as_deref().is_some_and(|m| !is_hint(m));
    let with_client_model = runs.iter().filter(reported).count();
    let hint_only = runs
        .iter()
        .filter(|r| r.model.as_deref().is_some_and(is_hint))
        .count();
    let none = total.saturating_sub(with_client_model + hint_only);
    let with_cost = runs
        .iter()
        .filter(|r| r.cost.is_some_and(|c| c.is_finite() && c > 0.0))
        .count();
    let mut by_model: BTreeMap<String, ModelRow> = BTreeMap::new();
    for r in runs {
        if r.model.as_deref().is_some_and(is_hint) {
            continue;
        }
        let key = format!(
            "{}/{}",
            r.client.as_deref().unwrap_or("unknown"),
            r.model.as_deref().unwrap_or("unknown")
        );
        let e = by_model.entry(key).or_default();
        e.runs += 1;
        if r.cost.is_some_and(|c| c.is_finite() && c > 0.0) {
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
        hint_only,
        none,
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
        assert_eq!((m.hint_only, m.none), (0, 1));
        assert_eq!((m.unknown_model_pct, m.unknown_cost_pct), (33.3, 66.7));
        let opus = &m.by_model["claude/opus"];
        assert_eq!((opus.runs, opus.unknown_cost_pct), (1, 0.0));
        assert!((opus.cost - 2.17).abs() < 1e-9);
        let bare = &m.by_model["unknown/unknown"];
        assert_eq!((bare.runs, bare.unknown_cost_pct), (1, 100.0));
    }

    #[test]
    fn size_hint_counts_as_hint_only_not_a_model() {
        for hint in ["small", "M", "large", "S", "L", "medium", "SMALL"] {
            assert!(is_hint(hint), "{hint}");
        }
        for real in [
            "opus",
            "sonnet",
            "haiku",
            "opencode-go/m",
            "claude-sonnet-5-5",
            "",
        ] {
            assert!(!is_hint(real), "{real}");
        }
        let m = models(&[
            run(Some("claude"), Some("small"), None),
            run(Some("claude"), Some("M"), None),
            run(None, Some("large"), None),
            run(Some("claude"), Some("opus"), Some(1.0)),
            run(None, None, None),
        ]);
        assert_eq!((m.total, m.with_client_model), (5, 1));
        assert_eq!((m.hint_only, m.none), (3, 1));
        assert_eq!(m.unknown_model_pct, 80.0);
        assert!(
            !m.by_model
                .keys()
                .any(|k| { k.ends_with("/small") || k.ends_with("/M") || k.ends_with("/large") })
        );
        assert_eq!(m.by_model["claude/opus"].runs, 1);
        assert_eq!(m.by_model["unknown/unknown"].runs, 1);
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
            run(Some("opencode"), Some("opencode-go/m"), Some(f64::INFINITY)),
        ]);
        assert_eq!((m.with_cost, m.unknown_cost_pct), (1, 75.0));
        assert_eq!(m.by_model["opencode/opencode-go/m"].unknown_cost_pct, 75.0);
    }
}
