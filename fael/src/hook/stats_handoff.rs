//! The after-handoff line of `fael stats` (issue 01M5Y9KQ): two groups of runs
//! side by side, counts only — no percentages, no verdict. The groups differ in
//! ways fael cannot see, so the line says what was counted, never what it did.

use crate::core::stats::{HandoffRuns, Side};

fn side(name: &str, s: &Side) -> String {
    format!(
        "{name} {} (done {} · re-finds {}/{} finds · rejects {})",
        s.runs, s.done, s.refinds, s.finds, s.rejects
    )
}

/// `None` when neither group has a run.
pub(super) fn handoff_line(h: &HandoffRuns) -> Option<String> {
    (h.with.runs + h.without.runs > 0).then(|| {
        format!(
            "  after-handoff runs: {} · {}",
            side("with", &h.with),
            side("without", &h.without)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_has_one_format_and_is_silent_without_runs() {
        assert_eq!(handoff_line(&HandoffRuns::default()), None);
        let h = HandoffRuns {
            with: Side {
                runs: 57,
                joined: 50,
                done: 47,
                finds: 183,
                refinds: 42,
                rejects: 84,
            },
            without: Side {
                runs: 77,
                joined: 70,
                done: 62,
                finds: 187,
                refinds: 48,
                rejects: 84,
            },
        };
        assert_eq!(
            handoff_line(&h).unwrap(),
            "  after-handoff runs: with 57 (done 47 · re-finds 42/183 finds · rejects 84) · without 77 (done 62 · re-finds 48/187 finds · rejects 84)"
        );
    }
}
