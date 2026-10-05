//! The validation checkpoint (SPEC-fael-learn-loop §E, PLAN chunk 5): the
//! candidate arm, where the gate really cut, against the holdout, where
//! `baseline@1` ran. Every number the verdict reads is a threshold frozen
//! before any data (decision 01M45DKB4) — this file only applies them. It
//! reports one of `validated` / `not_validated` / `insufficient_data` and the
//! reasons; filing the decision row, and opening the policy, stay with a human.

use super::cover::Coverage;
use super::rate::Rate;
use super::{Ob, Retained, STRATUM_MIN_PUSHES, STRATUM_MIN_SESSIONS, replay, result};
use crate::query::{ARM_HOLDOUT, TOUCH};
use crate::stats::outcomes::Observation;
use crate::stats::parse::Parsed;
use serde::Serialize;
use std::collections::BTreeMap;

/// Trigger shares of the two arms may differ by this many points (a guard that
/// passes is not proof the arms compare).
pub const MAX_TRIGGER_GAP_PP: f64 = 10.0;
/// Over the usable strata, each arm needs this much …
pub const MIN_SESSIONS: usize = 30;
pub const MIN_PUSHES: usize = 300;
/// … and the candidate arm this many gate cuts (one per session and row).
pub const MIN_GATE_CUTS: usize = 200;
/// `validated`: exposure at least this much lower …
pub const MIN_EXPOSURE_CUT_PCT: f64 = 40.0;
/// … each outcome kept at least this much (the holdout replay) …
pub const MIN_RETAINED_PCT: f64 = 85.0;
/// … `missed_push / gate cuts` at most this at its 95% upper bound …
pub const MAX_MISSED_HI_PCT: f64 = 2.0;
/// … and sessions that went back for a cut row no more than this much above
/// the holdout's.
pub const MAX_RETRIEVED_OVER_PCT: f64 = 20.0;

/// One arm of one stratum (or all usable strata pooled).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ArmSize {
    pub sessions: usize,
    pub search_pushes: usize,
    pub rows_said: usize,
    /// Rows the gate cut, one per session and row.
    pub gate_cuts: usize,
    /// Gate cuts the agent pulled itself and then cited or acted on.
    pub missed_push: Rate,
    /// Sessions that pulled a cut row themselves (any cut but the shadow).
    pub retrieved_sessions: Rate,
    /// The same, per row — the push-level diagnostic.
    pub retrieved_rows: usize,
    pub triggers: BTreeMap<String, usize>,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Used,
    /// Too few sessions or search pushes in an arm: reported, never a failure.
    Insufficient,
    /// The arms' trigger mixes differ too much to compare.
    Unbalanced,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StratumArms {
    pub repo: String,
    pub client: String,
    pub status: Status,
    pub max_trigger_gap_pp: f64,
    pub candidate: ArmSize,
    pub holdout: ArmSize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verdict {
    /// `validated` · `not_validated` · `insufficient_data`.
    pub result: &'static str,
    /// What missed, or what is missing; empty when validated.
    pub why: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Validation {
    pub candidate: String,
    pub strata: Vec<StratumArms>,
    pub candidate_all: ArmSize,
    pub holdout_all: ArmSize,
    /// Rows said per session, candidate against holdout, weighted over the
    /// usable strata by their search pushes.
    pub exposure_cut_pct: Option<f64>,
    /// What the gate forfeits: the candidate replayed on the holdout's rows.
    pub retained: Retained,
    /// Sessions that went back for a cut row, percent — weighted likewise.
    pub retrieved_pct: (Option<f64>, Option<f64>),
    /// The session view and the push view point opposite ways.
    pub warning: Option<String>,
    pub notes: Vec<String>,
    pub verdict: Verdict,
}

/// Largest gap, in points, between the arms' trigger shares.
fn trigger_gap(c: &ArmSize, h: &ArmSize) -> f64 {
    let share = |a: &ArmSize, t: &str| {
        100.0 * *a.triggers.get(t).unwrap_or(&0) as f64 / a.search_pushes.max(1) as f64
    };
    c.triggers
        .keys()
        .chain(h.triggers.keys())
        .map(|t| (share(c, t) - share(h, t)).abs())
        .fold(0.0, f64::max)
}

fn pct(r: &Rate) -> Option<f64> {
    r.pct()
}

/// Per-stratum values weighted by that stratum's search pushes.
fn weighted(parts: &[(f64, Option<f64>)]) -> Option<f64> {
    let (w, s) = parts
        .iter()
        .filter_map(|(w, v)| Some((*w, v.as_ref()? * w)))
        .fold((0.0, 0.0), |(a, b), (w, x)| (a + w, b + x));
    (w > 0.0).then(|| s / w)
}

fn sum(a: Rate, b: Rate) -> Rate {
    Rate::new(a.x + b.x, a.n + b.n)
}

/// The verdict, from the pooled numbers alone — the frozen rule, applied.
fn decide(v: &Validation, used: usize) -> Verdict {
    let (c, h) = (&v.candidate_all, &v.holdout_all);
    let mut missing = vec![];
    if v.candidate.contains(',') {
        missing.push(format!(
            "the candidate policy changed in the window ({})",
            v.candidate
        ));
    }
    if used < 2 {
        missing.push(format!("usable strata {used} < 2"));
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
    let mut failed = vec![];
    match v.exposure_cut_pct {
        Some(e) if e >= MIN_EXPOSURE_CUT_PCT => {}
        e => failed.push(format!(
            "exposure down {} < {MIN_EXPOSURE_CUT_PCT}%",
            e.map_or("—".into(), |e| format!("{e:.0}%"))
        )),
    }
    let r = &v.retained;
    for (name, rate) in [
        ("cited", &r.cited),
        ("pulled", &r.pulled),
        ("acted", &r.acted),
    ] {
        // no event of a kind = nothing to keep or lose, said in the report
        if let Some(p) = pct(rate).filter(|p| *p < MIN_RETAINED_PCT) {
            failed.push(format!("{name} retained {p:.0}% < {MIN_RETAINED_PCT}%"));
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
    if let (Some(cp), Some(hp)) = v.retrieved_pct
        && cp > hp * (1.0 + MAX_RETRIEVED_OVER_PCT / 100.0)
    {
        failed.push(format!(
            "sessions going back for a cut row {cp:.1}% vs holdout {hp:.1}%, over +{MAX_RETRIEVED_OVER_PCT}%"
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

/// `None` when no push carries an arm: no experiment, nothing to report.
pub(super) fn validate(
    parsed: &Parsed,
    seen: &[Observation],
    obs: &[Ob],
    tz_min: i32,
) -> Option<Validation> {
    let (by, policies) = gather::gather(parsed, seen, tz_min)?;
    let mut strata = vec![];
    let (mut pc, mut ph) = (Acc::default(), Acc::default());
    let (mut exposure, mut retrieved) = (vec![], (vec![], vec![]));
    let mut retained = Retained::default();
    for ((repo, client), [c, h]) in &by {
        let (cs, hs) = (c.size(), h.size());
        let gap = trigger_gap(&cs, &hs);
        let eligible = [&cs, &hs]
            .iter()
            .all(|a| a.sessions >= STRATUM_MIN_SESSIONS && a.search_pushes >= STRATUM_MIN_PUSHES);
        let status = match (eligible, gap > MAX_TRIGGER_GAP_PP) {
            (false, _) => Status::Insufficient,
            (true, true) => Status::Unbalanced,
            (true, false) => Status::Used,
        };
        if status == Status::Used {
            pc.merge(c);
            ph.merge(h);
            let w = (cs.search_pushes + hs.search_pushes) as f64;
            let per = |a: &ArmSize| a.rows_said as f64 / a.sessions as f64;
            exposure.push((
                w,
                (per(&hs) > 0.0).then(|| 100.0 * (1.0 - per(&cs) / per(&hs))),
            ));
            retrieved.0.push((w, pct(&cs.retrieved_sessions)));
            retrieved.1.push((w, pct(&hs.retrieved_sessions)));
            // what the gate forfeits is read off the holdout, which saw every row
            let held: Vec<&Ob> = obs
                .iter()
                .filter(|o| o.arm == ARM_HOLDOUT && (o.o.repo, o.client) == (*repo, *client))
                .collect();
            let r = result(TOUCH.name(), &held, &replay::touch(&held)).retained;
            retained = Retained {
                cited: sum(retained.cited, r.cited),
                pulled: sum(retained.pulled, r.pulled),
                acted: sum(retained.acted, r.acted),
            };
        }
        strata.push(StratumArms {
            repo: repo.to_string(),
            client: client.to_string(),
            status,
            max_trigger_gap_pp: gap,
            candidate: cs,
            holdout: hs,
        });
    }
    let used = strata.iter().filter(|s| s.status == Status::Used).count();
    let (candidate_all, holdout_all) = (pc.size(), ph.size());
    let retrieved_pct = (weighted(&retrieved.0), weighted(&retrieved.1));
    let rows = |a: &ArmSize| a.retrieved_rows as f64 / a.search_pushes.max(1) as f64;
    let warning = match (retrieved_pct, rows(&candidate_all) - rows(&holdout_all)) {
        ((Some(c), Some(h)), push) if (c - h) * push < 0.0 => Some(format!(
            "the session view ({c:.1}% vs {h:.1}%) and the push view ({:.3} vs {:.3} rows per push) point opposite ways: read both before filing",
            rows(&candidate_all),
            rows(&holdout_all)
        )),
        _ => None,
    };
    let mut v = Validation {
        candidate: policies.into_iter().collect::<Vec<_>>().join(","),
        strata,
        candidate_all,
        holdout_all,
        exposure_cut_pct: weighted(&exposure),
        retained,
        retrieved_pct,
        warning,
        notes: vec![
            "dup is not measured (SPEC §B): the session-level safety gate rests on retrieved_after_cut alone".into(),
            "retained is the candidate replayed on holdout rows, an upper bound: a cut row may be said at a later push".into(),
        ],
        verdict: Verdict {
            result: "insufficient_data",
            why: vec![],
        },
    };
    v.verdict = decide(&v, used);
    Some(v)
}

mod gather;
use gather::Acc;
#[cfg(test)]
mod tests;
