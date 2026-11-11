//! The `models` card of the human `fael stats` text — format only, every
//! number comes from `fael-core::stats`. Split out of `stats_text.rs`
//! (file-size ratchet); `--json` is the contract, this text is unstable.

use crate::core;
use std::collections::BTreeMap;

/// `models: N runs · reported a · hint only b · none c` plus one line per
/// `client/model` group (hint-model rows excluded — they count under
/// `hint only`, never under a model) — nothing at all when the clone has
/// no crew.db runs. The unknown-model share is `(b + c) / N`: hints count
/// as unknown.
pub(super) fn models_lines(m: &core::stats::Models) -> Vec<String> {
    if m.total == 0 {
        return Vec::new();
    }
    let mut out = vec![format!(
        "  models: {} runs · reported {} · hint only {} · none {}",
        m.total, m.with_client_model, m.hint_only, m.none
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
fn real_suffix(real: &BTreeMap<String, u64>, client: &str) -> String {
    real.get(client)
        .map(|n| format!(", ~{n} real"))
        .unwrap_or_default()
}

/// One line per client (count, cost, measured real tokens beside it) plus
/// the `models` card — the whole attribution block of `fael stats`.
pub(super) fn print_attribution(s: &core::stats::Stats) {
    let mut cl: Vec<_> = s.by_client.iter().collect();
    cl.sort_by_key(|a| std::cmp::Reverse(a.1.events));
    for (k, c) in cl {
        let real = real_suffix(&s.real_by_client, k);
        println!(
            "  client {k}: ×{} (~{} tokens{real})",
            c.events, c.est_tokens
        );
    }
    for line in models_lines(&s.models) {
        println!("{line}");
    }
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
            "  models: 2 runs · reported 1 · hint only 0 · none 1"
        );
        assert!(
            lines.contains(&"  model claude/opus: 1 runs $2.170000 (unknown 0.0%)".to_string())
        );
        assert!(
            lines.contains(
                &"  model unknown/unknown: 1 runs $0.000000 (unknown 100.0%)".to_string()
            )
        );
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
