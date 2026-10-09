//! The `experience` block of `fael stats` (PLAN-fael-experience-loop chunk
//! 4): capture → acted → outcome, plus capture recall. It reads numbers other
//! blocks already hold (`said.finding`, `said.check`, `capture`,
//! `context_loop`) beside `experience`'s own, so nothing is counted twice.

use crate::core;

/// The block's lines; nothing counted = none.
pub(super) fn experience_lines(s: &core::stats::Stats) -> Vec<String> {
    let (e, c, l) = (&s.experience, &s.capture, &s.context_loop);
    let said = |k: &str| s.said.get(k).cloned().unwrap_or_default();
    let (finding, check) = (said("finding"), said("check"));
    let none = finding.said
        + check.said
        + c.edit_session_issues
        + e.fixed
        + e.closed_with_check
        + l.confirmed_repeats
        + e.fix_commits
        == 0;
    if none {
        return vec![];
    }
    vec![
        // only a review whose model calls ReportFindings is seen: a review
        // that prints its findings as text is not (01M4F5QDH), so the count is a floor
        format!(
            "  experience — capture: review findings {} (ReportFindings calls only) → issues {} · issues {} in {} edit sessions",
            finding.said, finding.earned, c.edit_session_issues, c.sessions_with_edits
        ),
        format!(
            "    acted: fixed {} (after a review finding {}) · closed with a check {}",
            e.fixed, e.fixed_from_review, e.closed_with_check
        ),
        format!(
            "    outcome: repeats {} of {} edits after close with a check · {} of {} without · gone checks said {}",
            e.repeats_with_check,
            e.edits_after_close_with_check,
            l.confirmed_repeats - e.repeats_with_check,
            l.edits_after_close - e.edits_after_close_with_check,
            check.said
        ),
        format!(
            "    recall: fix commits linked to fael {} of {}",
            e.fix_commits_linked, e.fix_commits
        ),
    ]
}

/// The label contract's measures (PLAN-fael-label chunk 3): `num/den (pct)`,
/// `n/a (why)` with no base — never 0% for a missing one. Nothing keyed or
/// closed since the contract = no line.
pub(super) fn label_line(s: &core::stats::Stats) -> Option<String> {
    use core::stats::{Measure, State};
    let l = &s.experience.label;
    if l.close_core.den + l.key_reuse.den == 0 {
        return None;
    }
    let show = |m: &Measure, none: &str| match m.state {
        State::Unmeasurable => format!("n/a ({none})"),
        State::Measured => format!("{}/{} ({}%)", m.num, m.den, m.num * 100 / m.den),
    };
    let closed = "no issue closed since";
    let gone = match l.gone_repos {
        0 => String::new(),
        n => format!(" · {n} repo path(s) gone, read through a live checkout if any"),
    };
    Some(format!(
        "  label — close core {} · guard {} · key reuse {} · find hit {} — since {}{gone}",
        show(&l.close_core, closed),
        show(&l.guard, closed),
        show(&l.key_reuse, "no keyed add since"),
        show(&l.find_hit, "usage keeps no missed find"),
        core::stats::LABEL_SINCE.get(..10).unwrap_or_default()
    ))
}
