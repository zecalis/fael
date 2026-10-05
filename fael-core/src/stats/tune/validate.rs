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
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

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
    /// Sessions whose new row self-heal linked to a row the session was never
    /// shown (SPEC §B `dup`) — the writer is `hook::tally::note_filed`.
    pub dup_sessions: Rate,
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

/// The primary bars' inputs for one stratum, or for a repo's used strata pooled.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Bars {
    /// Rows said per session, candidate against holdout, percent down.
    pub exposure_cut_pct: Option<f64>,
    /// What the gate forfeits: the candidate replayed on the holdout's rows.
    pub retained: Retained,
    /// Sessions that went back for a cut row, percent: candidate, holdout.
    pub retrieved_pct: (Option<f64>, Option<f64>),
    /// Sessions that filed a row duplicating one they were never shown, percent.
    pub dup_pct: (Option<f64>, Option<f64>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StratumArms {
    /// The repo scope (SPEC §E), not the worktree path of a usage line.
    pub repo: String,
    pub client: String,
    pub status: Status,
    pub max_trigger_gap_pp: f64,
    pub candidate: ArmSize,
    pub holdout: ArmSize,
    /// Only a used stratum is compared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bars: Option<Bars>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verdict {
    /// `validated` · `not_validated` · `insufficient_data`.
    pub result: &'static str,
    /// What missed, or what is missing; empty when validated.
    pub why: Vec<String>,
}

impl Verdict {
    pub fn insufficient(why: &str) -> Verdict {
        Verdict {
            result: "insufficient_data",
            why: vec![why.into()],
        }
    }
}

/// One repo's checkpoint: its own strata, its own pool, its own verdict.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Validation {
    pub repo: String,
    pub candidate: String,
    pub strata: Vec<StratumArms>,
    pub candidate_all: ArmSize,
    pub holdout_all: ArmSize,
    /// The used strata pooled, weighted by their search pushes.
    #[serde(flatten)]
    pub bars: Bars,
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

/// One validation per repo scope; none while no push carries an arm. `scope`
/// maps a usage line's `repo` to its repo (resolved once per distinct value).
pub(super) fn validate(
    parsed: &Parsed,
    seen: &[Observation],
    obs: &[Ob],
    tz_min: i32,
    scope: &dyn Fn(&str) -> String,
) -> Vec<Validation> {
    let memo = RefCell::new(HashMap::<String, String>::new());
    let scope = |r: &str| {
        let mut m = memo.borrow_mut();
        m.entry(r.to_string()).or_insert_with(|| scope(r)).clone()
    };
    let Some(by) = gather::gather(parsed, seen, tz_min, &scope) else {
        return vec![];
    };
    by.into_iter()
        .map(|(repo, r)| {
            let held = |client: &str| -> Vec<&Ob> {
                let of =
                    |o: &&Ob| o.arm == ARM_HOLDOUT && o.client == client && scope(o.o.repo) == repo;
                obs.iter().filter(of).collect()
            };
            one(repo.clone(), r, held)
        })
        .collect()
}

/// A stratum's primary-bar inputs; the retained part is read off the holdout,
/// which saw every row.
fn bars(cs: &ArmSize, hs: &ArmSize, held: &[&Ob]) -> Bars {
    let per = |a: &ArmSize| a.rows_said as f64 / a.sessions as f64;
    Bars {
        exposure_cut_pct: (per(hs) > 0.0).then(|| 100.0 * (1.0 - per(cs) / per(hs))),
        retained: result(TOUCH.name(), held, &replay::touch(held)).retained,
        retrieved_pct: (pct(&cs.retrieved_sessions), pct(&hs.retrieved_sessions)),
        dup_pct: (pct(&cs.dup_sessions), pct(&hs.dup_sessions)),
    }
}

fn one<'a>(repo: String, r: RepoArms<'a>, held: impl Fn(&str) -> Vec<&'a Ob<'a>>) -> Validation {
    let mut strata = vec![];
    let (mut pc, mut ph) = (Acc::default(), Acc::default());
    let (mut exposure, mut retrieved, mut dup) = (vec![], (vec![], vec![]), (vec![], vec![]));
    let mut retained = Retained::default();
    for (client, [c, h]) in &r.clients {
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
        let mut b = None;
        if status == Status::Used {
            pc.merge(c);
            ph.merge(h);
            let s = bars(&cs, &hs, &held(client));
            let w = (cs.search_pushes + hs.search_pushes) as f64;
            exposure.push((w, s.exposure_cut_pct));
            retrieved.0.push((w, s.retrieved_pct.0));
            retrieved.1.push((w, s.retrieved_pct.1));
            dup.0.push((w, s.dup_pct.0));
            dup.1.push((w, s.dup_pct.1));
            retained = Retained {
                cited: sum(retained.cited, s.retained.cited),
                pulled: sum(retained.pulled, s.retained.pulled),
                acted: sum(retained.acted, s.retained.acted),
            };
            b = Some(s);
        }
        strata.push(StratumArms {
            repo: repo.clone(),
            client: client.to_string(),
            status,
            max_trigger_gap_pp: gap,
            candidate: cs,
            holdout: hs,
            bars: b,
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
        repo,
        candidate: r.policies.into_iter().collect::<Vec<_>>().join(","),
        strata,
        candidate_all,
        holdout_all,
        bars: Bars {
            exposure_cut_pct: weighted(&exposure),
            retained,
            retrieved_pct,
            dup_pct: (weighted(&dup.0), weighted(&dup.1)),
        },
        warning,
        notes: vec![
            "dup counts only rows filed after the writer existed (SPEC §B): no `dup` line yet reads as 0%, not as proof".into(),
            "retained is the candidate replayed on holdout rows, an upper bound: a cut row may be said at a later push".into(),
        ],
        verdict: Verdict {
            result: "insufficient_data",
            why: vec![],
        },
    };
    v.verdict = decide(&v, used);
    v
}

mod decide;
mod gather;
mod shadow;
use decide::decide;
use gather::{Acc, RepoArms};
pub use shadow::shadow_verdict;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_scope;
#[cfg(test)]
mod tests_shadow;
