//! The shadow stage's verdict (PLAN-fael-learn-loop chunk 6): `touch@1`
//! replayed on a repo's shadow pushes — every session saw everything, so the
//! replay is on the rows actually said — against the same exposure and
//! retained bars and the repo minimums. No arm exists to compare, so nothing
//! about `missed_push`, `retrieved_after_cut` or `dup` is judged here.

use super::super::Section;
use super::decide::misses;
use super::{Bars, MIN_GATE_CUTS, MIN_PUSHES, MIN_SESSIONS, Verdict};
use crate::query::TOUCH;

/// `validated` = promote to canary · `not_validated` = roll back to baseline ·
/// `insufficient_data` = stay in shadow and keep collecting.
pub fn shadow_verdict(all: &Section) -> Verdict {
    let Some(p) = all.policies.iter().find(|p| p.policy == TOUCH.name()) else {
        return Verdict {
            result: "insufficient_data",
            why: vec!["no replay of touch@1".into()],
        };
    };
    let z = &all.sizes;
    let mut missing = vec![];
    if z.sessions < MIN_SESSIONS || z.search_pushes < MIN_PUSHES {
        missing.push(format!(
            "{} sessions / {} search pushes, want ≥ {MIN_SESSIONS} / ≥ {MIN_PUSHES}",
            z.sessions, z.search_pushes
        ));
    }
    if p.dropped < MIN_GATE_CUTS {
        missing.push(format!("would-drop rows {} < {MIN_GATE_CUTS}", p.dropped));
    }
    if !all.coverage.passes {
        missing.push("outside the coverage thresholds".into());
    }
    if !missing.is_empty() {
        return Verdict {
            result: "insufficient_data",
            why: missing,
        };
    }
    let bars = Bars {
        exposure_cut_pct: p.exposure.pct().map(|e| 100.0 - e),
        retained: p.retained.clone(),
        ..Bars::default()
    };
    let why = misses(&bars, "");
    Verdict {
        result: if why.is_empty() {
            "validated"
        } else {
            "not_validated"
        },
        why,
    }
}
