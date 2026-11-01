//! The human outcome line for `fael stats` — format only, every number from
//! `fael-core::stats`. Split out of `stats_text.rs` (file-size cap); `--json`
//! is the contract, this text is unstable by design. Observed joins only: a
//! PR carrying a fael row is linked, never caused.

use crate::core;

/// Whole seconds as the largest whole unit — `90` → `1m`, `7200` → `2h`.
fn dur(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3_600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// `outcome: 594 PRs merged · 200 listed since … (41 with row · 159 without)
/// · …` beside the value line — nothing observed = no line. Flow coverage
/// and the latest throughput week ride the same line (JSON-only before);
/// `value`/`by_event` text is untouched.
pub(super) fn outcome_line(s: &core::stats::Stats) -> Option<String> {
    let o = &s.outcome;
    if o.prs_merged == 0 && o.prs_created == 0 && o.issues_filed == 0 && o.issues_closed == 0 {
        return None;
    }
    let mut segs = vec![if o.prs_merged_unknown {
        "PRs merged unknown (offline)".to_string()
    } else {
        format!("{} PRs merged", o.prs_merged)
    }];
    let listed = o.prs_with_row + o.prs_without_row;
    if listed > 0 {
        let since = if o.joined_since.is_empty() {
            String::new()
        } else {
            format!(" since {}", o.joined_since)
        };
        segs.push(format!(
            "{listed} listed{since} ({} with row · {} without)",
            o.prs_with_row, o.prs_without_row
        ));
    }
    if o.issues_filed + o.issues_closed > 0 {
        segs.push(format!(
            "issues {} filed / {} closed",
            o.issues_filed, o.issues_closed
        ));
    }
    if o.filed_to_close.n > 0 {
        segs.push(format!(
            "filed→close med {} p90 {} (n={})",
            dur(o.filed_to_close.median),
            dur(o.filed_to_close.p90),
            o.filed_to_close.n
        ));
    }
    if s.flow.runs_total > 0 {
        let cover = format!("{}/{} runs with PR", s.flow.runs_with_pr, s.flow.runs_total);
        segs.push(if s.flow.done_to_merge.n > 0 {
            format!(
                "done→merge med {} p90 {} (n={} · {cover})",
                dur(s.flow.done_to_merge.median),
                dur(s.flow.done_to_merge.p90),
                s.flow.done_to_merge.n
            )
        } else {
            format!("done→merge n=0 ({cover})")
        });
    }
    if let Some(last) = s.throughput.last() {
        segs.push(format!("{} dones week of {}", last.done, last.week));
    }
    Some(format!("outcome: {}", segs.join(" · ")))
}

#[cfg(test)]
mod tests {
    use super::outcome_line;
    use crate::core::stats::{Flow, Outcome, Span, Stats, WeekCount};

    fn stats_with(outcome: Outcome, flow: Flow, throughput: Vec<WeekCount>) -> Stats {
        // SAFETY: tests only — build a Stats through the real join over empty input.
        let parsed = crate::core::stats::parse("", std::path::Path::new("/w/u.jsonl"), &[]);
        let mut s = crate::core::stats::aggregate(
            &parsed,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &[],
            &[],
            &crate::core::Config::default(),
            (0, 0, 0, 0).into(),
            false,
            0,
            None,
            &[],
            None,
        );
        s.outcome = outcome;
        s.flow = flow;
        s.throughput = throughput;
        s
    }

    #[test]
    fn nothing_observed_is_no_line() {
        assert_eq!(
            outcome_line(&stats_with(Outcome::default(), Flow::default(), vec![])),
            None
        );
    }

    #[test]
    fn joins_print_beside_joined_since_and_flows_beside_coverage() {
        let o = Outcome {
            prs_merged: 594,
            prs_created: 200,
            issues_filed: 12,
            issues_closed: 9,
            filed_to_close: Span {
                median: 7200,
                p90: 32400,
                n: 9,
            },
            joined_since: "2026-10-19".into(),
            prs_with_row: 41,
            prs_without_row: 159,
            ..Default::default()
        };
        let f = Flow {
            done_to_merge: Span {
                median: 1800,
                p90: 3600,
                n: 3,
            },
            runs_with_pr: 105,
            runs_total: 136,
            ..Default::default()
        };
        let t = vec![WeekCount {
            week: "2026-10-26".into(),
            done: 4,
        }];
        assert_eq!(
            outcome_line(&stats_with(o, f, t)).as_deref(),
            Some(
                "outcome: 594 PRs merged · 200 listed since 2026-10-19 (41 with row · 159 without) \
                 · issues 12 filed / 9 closed · filed→close med 2h p90 9h (n=9) \
                 · done→merge med 30m p90 1h (n=3 · 105/136 runs with PR) · 4 dones week of 2026-10-26"
            )
        );
    }

    #[test]
    fn offline_reads_unknown_beside_the_ledger_it_did_read() {
        let o = Outcome {
            prs_merged_unknown: true,
            issues_filed: 2,
            ..Default::default()
        };
        let line = outcome_line(&stats_with(o, Flow::default(), vec![])).unwrap();
        assert!(
            line.starts_with("outcome: PRs merged unknown (offline) · issues 2 filed / 0 closed")
        );
    }
}
