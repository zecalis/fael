//! The row lifecycle panel (decision 01M5528X): of this repo's rows, how many are
//! live, held, dormant or retired, per kind. Pure: the adapter reads the disk and
//! hands over the log with `dormant`/`retired` filled and the ids it found held.

use crate::{Log, closed, superseded};
use serde::Serialize;
use std::collections::HashSet;

/// One kind's rows by state; the four add up to every row of the kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct States {
    /// open, no stamped file changed (or no verdict)
    pub live: usize,
    /// open, some but not all stamped files changed — what the Held line asks about
    pub held: usize,
    /// open, every stamped file changed and no bump since (hidden, a bump revives it)
    pub dormant: usize,
    /// closed, superseded, or a note whose chunk is done or branch merged
    pub retired: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Lifecycle {
    pub decision: States,
    pub issue: States,
    pub note: States,
    /// issues filed `from: report` that are closed — audit accuracy, read later
    #[serde(skip_serializing_if = "is_zero")]
    pub report_closed: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Lifecycle {
    pub fn is_empty(&self) -> bool {
        *self == Lifecycle::default()
    }
}

/// Where one row sits; the one rule behind `lifecycle` and the decision yield.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    Live,
    Held,
    Dormant,
    Retired,
}

pub(super) fn state_of(
    r: &crate::Row,
    gone: &HashSet<&str>,
    log: &Log,
    held: &HashSet<String>,
) -> State {
    if gone.contains(r.id.as_str()) || log.retired.contains(&r.id) {
        State::Retired
    } else if log.dormant.contains(&r.id) {
        State::Dormant
    } else if held.contains(&r.id) {
        State::Held
    } else {
        State::Live
    }
}

/// `held` = ids of open rows whose stamped files changed in part. A derived-retired
/// note is in `log.dormant` too; it counts retired, once.
pub fn lifecycle(log: &Log, held: &HashSet<String>) -> Lifecycle {
    let gone: HashSet<&str> = closed(log).union(&superseded(log)).copied().collect();
    let mut out = Lifecycle::default();
    for r in &log.rows {
        let t = match r.kind.as_str() {
            "decision" => &mut out.decision,
            "issue" => &mut out.issue,
            "note" => &mut out.note,
            _ => continue,
        };
        let slot = match state_of(r, &gone, log, held) {
            State::Retired => &mut t.retired,
            State::Dormant => &mut t.dormant,
            State::Held => &mut t.held,
            State::Live => &mut t.live,
        };
        *slot += 1;
        if r.kind == "issue" && r.from_report() && gone.contains(r.id.as_str()) {
            out.report_closed += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Row;

    fn row(id: &str, kind: &str) -> Row {
        Row {
            id: id.into(),
            kind: kind.into(),
            ..Default::default()
        }
    }

    #[test]
    fn each_row_lands_in_exactly_one_state() {
        let mut log = Log::default();
        for (id, kind) in [
            ("a", "decision"),
            ("b", "decision"),
            ("c", "decision"),
            ("d", "note"),
            ("e", "note"),
            ("f", "issue"),
        ] {
            log.rows.push(row(id, kind));
        }
        log.dormant = ["b", "e"].map(String::from).into();
        log.retired = ["e"].map(String::from).into();
        let held: HashSet<String> = ["c".to_string()].into();
        let l = lifecycle(&log, &held);
        assert_eq!(
            l.decision,
            States {
                live: 1,
                held: 1,
                dormant: 1,
                retired: 0
            }
        );
        assert_eq!(
            l.note,
            States {
                live: 1,
                held: 0,
                dormant: 0,
                retired: 1
            }
        );
        assert_eq!(l.issue.live, 1);
    }

    #[test]
    fn closed_report_issues_are_counted() {
        let mut log = Log::default();
        for (id, kind, from) in [
            ("a", "issue", Some("report")),
            ("b", "issue", Some("report")),
            ("c", "issue", None),
            ("d", "note", Some("report")),
        ] {
            log.rows.push(Row {
                from: from.map(String::from),
                ..row(id, kind)
            });
        }
        log.closes.push(Row {
            reference: Some("a".into()),
            ..Row::default()
        });
        log.closes.push(Row {
            reference: Some("c".into()),
            ..Row::default()
        });
        assert_eq!(lifecycle(&log, &HashSet::new()).report_closed, 1);
    }
}
