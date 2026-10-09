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
