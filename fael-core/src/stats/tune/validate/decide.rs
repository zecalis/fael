//! The frozen rule, applied: one repo's numbers in, one verdict out.

use super::*;

/// The primary bars one set of numbers misses; `who` prefixes each reason.
/// `missed_push` is not here: it is judged on the repo pool only (SPEC §E).
pub(super) fn misses(b: &Bars, who: &str) -> Vec<String> {
    let mut failed = vec![];
    match b.exposure_cut_pct {
        Some(e) if e >= MIN_EXPOSURE_CUT_PCT => {}
        e => failed.push(format!(
            "{who}exposure down {} < {MIN_EXPOSURE_CUT_PCT}%",
            e.map_or("—".into(), |e| format!("{e:.0}%"))
        )),
    }
    let r = &b.retained;
    for (name, rate) in [
        ("cited", &r.cited),
        ("pulled", &r.pulled),
        ("acted", &r.acted),
    ] {
        // no event of a kind = nothing to keep or lose, said in the report
        if let Some(p) = pct(rate).filter(|p| *p < MIN_RETAINED_PCT) {
            failed.push(format!(
                "{who}{name} retained {p:.0}% < {MIN_RETAINED_PCT}%"
            ));
        }
    }
    for (what, (c, h)) in [
        ("sessions going back for a cut row", b.retrieved_pct),
        (
            "sessions filing a row that duplicates one never shown",
            b.dup_pct,
        ),
    ] {
        if let (Some(cp), Some(hp)) = (c, h)
            && cp > hp * (1.0 + MAX_RETRIEVED_OVER_PCT / 100.0)
        {
            failed.push(format!(
                "{who}{what} {cp:.1}% vs holdout {hp:.1}%, over +{MAX_RETRIEVED_OVER_PCT}%"
            ));
        }
    }
    failed
}

/// The repo's verdict. `used` strata are the eligible, balanced ones: the repo
/// needs one, and none of them may fail a primary bar on its own — a pool that
/// passes must not hide a client that does not.
pub(super) fn decide(v: &Validation, used: usize) -> Verdict {
    let (c, h) = (&v.candidate_all, &v.holdout_all);
    let mut missing = vec![];
    if v.candidate.contains(',') {
        missing.push(format!(
            "the candidate policy changed in the window ({})",
            v.candidate
        ));
    }
    if used < 1 {
        missing.push("no eligible stratum in this repo".to_string());
    }
    for (name, a) in [("candidate", c), ("holdout", h)] {
        if a.sessions < MIN_SESSIONS || a.search_pushes < MIN_PUSHES {
            missing.push(format!(
                "{name} arm {} sessions / {} search pushes, want ≥ {MIN_SESSIONS} / ≥ {MIN_PUSHES}",
                a.sessions, a.search_pushes
            ));
        }
        if !a.coverage.passes {
            missing.push(format!("{name} arm outside the coverage thresholds"));
        }
    }
    if c.gate_cuts < MIN_GATE_CUTS {
        missing.push(format!("gate cuts {} < {MIN_GATE_CUTS}", c.gate_cuts));
    }
    if !missing.is_empty() {
        return Verdict {
            result: "insufficient_data",
            why: missing,
        };
    }
    let mut failed = misses(&v.bars, "");
    // one stratum is the pool: its misses are already said
    if used > 1 {
        for s in v.strata.iter().filter(|s| s.status == Status::Used) {
            if let Some(b) = &s.bars {
                failed.extend(misses(b, &format!("client {}: ", s.client)));
            }
        }
    }
    if c.missed_push
        .hi
        .is_none_or(|hi| hi * 100.0 > MAX_MISSED_HI_PCT)
    {
        failed.push(format!(
            "missed_push upper bound above {MAX_MISSED_HI_PCT}% of gate cuts"
        ));
    }
    Verdict {
        result: if failed.is_empty() {
            "validated"
        } else {
            "not_validated"
        },
        why: failed,
    }
}
