//! The `models` card of the human `fael stats` text — format only, every
//! number comes from `fael-core::stats`. Split out of `stats_text.rs`
//! (file-size ratchet); `--json` is the contract, this text is unstable.

use crate::core;
use std::collections::BTreeMap;

/// `models: N runs · unknown model x% · unknown cost y%` plus one line per
/// `client/model` group — nothing at all when the clone has no crew.db runs.
pub(super) fn models_lines(m: &core::stats::Models) -> Vec<String> {
    if m.total == 0 {
        return Vec::new();
    }
    let mut out = vec![format!(
        "  models: {} runs · unknown model {:.1}% · unknown cost {:.1}%",
        m.total, m.unknown_model_pct, m.unknown_cost_pct
    )];
    let mut by: Vec<(&String, &core::stats::ModelRow)> = m.by_model.iter().collect();
    by.sort_by_key(|(_, r)| std::cmp::Reverse(r.runs));
    for (k, r) in by {
        out.push(format!(
            "  model {k}: {} runs ${:.6} (unknown {:.1}%)",
            r.runs, r.cost, r.unknown_cost_pct
        ));
    }
    out
}

/// `, ~R real` beside a client's cost when usage measured transcript tokens
/// for it — empty when none measured, so old output is byte-identical.
pub(super) fn real_suffix(real: &BTreeMap<String, u64>, client: &str) -> String {
    real.get(client)
        .map(|n| format!(", ~{n} real"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> core::stats::Models {
        core::stats::models(&[
            core::stats::ModelRun {
                client: Some("claude".into()),
                model: Some("opus".into()),
                cost: Some(2.17),
            },
            core::stats::ModelRun {
                client: None,
                model: None,
                cost: None,
            },
        ])
    }

    #[test]
    fn names_the_unknown_share_beside_every_number() {
        let lines = models_lines(&models());
        assert_eq!(
            lines[0],
            "  models: 2 runs · unknown model 50.0% · unknown cost 50.0%"
        );
        assert!(lines.contains(&"  model claude/opus: 1 runs $2.170000 (unknown 0.0%)".to_string()));
        assert!(lines.contains(
            &"  model unknown/unknown: 1 runs $0.000000 (unknown 100.0%)".to_string()
        ));
    }

    #[test]
    fn no_runs_is_no_line() {
        assert!(models_lines(&core::stats::Models::default()).is_empty());
        assert_eq!(real_suffix(&BTreeMap::new(), "claude"), "");
        assert_eq!(
            real_suffix(&BTreeMap::from([("claude".to_string(), 150)]), "claude"),
            ", ~150 real"
        );
    }
}
