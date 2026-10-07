//! The `fael stats` lines for the file-hash verdict (PLAN-fael-file-hash
//! chunk 4): what push could not say, and the retire rate the gate reads.

use crate::core;

/// What push could say about the shown rows' files (a count of what it could
/// not say, never of savings); no push measured it = no line. The second line
/// is the chunk 4 gate's input (PLAN-fael-file-hash §6.4), printed once a
/// pair's ask was said and a later edit set its deadline.
pub(super) fn verdict_line(v: &core::stats::FileVerdict) -> Option<String> {
    (v.changed + v.unchanged + v.no_verdict > 0).then(|| {
        let mut line = format!(
            "  file verdict at push: {} changed · {} unchanged · {} none ({} with no stamp, the rest over the 1 MiB push cap, gone or unfound)",
            v.changed, v.unchanged, v.no_verdict, v.no_fh
        );
        let (c, u) = (&v.retire.changed, &v.retire.unchanged);
        if c.pairs + u.pairs > 0 {
            let gate = match v.gate() {
                None => format!("gate waits for {} pairs a side", core::stats::GATE_MIN_PAIRS),
                Some(true) => "gate passes".into(),
                Some(false) => "gate fails".into(),
            };
            line += &format!(
                "\n  retired after the ask: changed {}/{} · unchanged {}/{} ({gate})",
                c.retired, c.pairs, u.retired, u.pairs
            );
        }
        line
    })
}

#[cfg(test)]
mod tests {
    use super::verdict_line;
    use crate::core::stats::{Arm, FileVerdict, RetireSplit};

    #[test]
    fn the_verdict_lines_say_what_push_could_not_and_when_the_gate_speaks() {
        assert_eq!(verdict_line(&FileVerdict::default()), None);
        let mut v = FileVerdict {
            changed: 3,
            unchanged: 1,
            no_verdict: 6,
            no_fh: 4,
            ..FileVerdict::default()
        };
        let line = verdict_line(&v).unwrap();
        assert!(
            line.contains("3 changed · 1 unchanged · 6 none (4 with no stamp"),
            "{line}"
        );
        // no pair with a said ask and a later edit yet: one line only
        assert!(!line.contains("retired after the ask"), "{line}");
        let arm = |pairs, retired| Arm { pairs, retired };
        v.retire = RetireSplit {
            changed: arm(2, 1),
            unchanged: arm(5, 0),
        };
        let line = verdict_line(&v).unwrap();
        assert!(
            line.contains("retired after the ask: changed 1/2 · unchanged 0/5 (gate waits for 30 pairs a side)"),
            "{line}"
        );
        v.retire = RetireSplit {
            changed: arm(30, 30),
            unchanged: arm(30, 0),
        };
        assert!(verdict_line(&v).unwrap().ends_with("(gate passes)"));
        // no lift over the unchanged side
        v.retire.unchanged = arm(30, 30);
        assert!(verdict_line(&v).unwrap().ends_with("(gate fails)"));
    }
}
