//! The brief-drift line (plan fael-chunk-drift): the owner edited the held chunk's brief,
//! scope or title after its run started. Said once per edit and session, with the ready
//! `fael chunk show`. Inform only; one read-only SQLite read, like `overlap`.

use super::protocol::Ctx;
use super::say::{Kind, Line};
use crate::core::work::{self, Drift};

/// The line for the worktree's held chunk, empty with no crew.db, no live run or no drift.
pub(crate) fn line(c: &Ctx) -> Option<Line> {
    let db = work::db_for(c.repo.journal.as_deref(), &c.repo.fael);
    work::drift(&db, &c.repo.root.to_string_lossy(), &c.session).map(|d| about(&d))
}

fn about(d: &Drift) -> Line {
    Line {
        kind: Kind::BriefDrift {
            key: format!("~drift:{}@{}", d.uid, d.event),
        },
        text: d.text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::say::Outbox;
    use crate::hook::state::lock_seen;

    fn drift(event: i64) -> Drift {
        Drift {
            uid: "U1".into(),
            event,
            fields: vec!["brief".into(), "scope".into()],
        }
    }

    #[test]
    fn says_once_per_edit_and_again_for_a_later_one() {
        let seen = crate::hook::say_contract::seen("s.seen");
        let say = |d: Drift| {
            let mut out = Outbox::open(lock_seen(&seen));
            out.say(about(&d));
            out.reply().context().map(String::from)
        };
        assert_eq!(
            say(drift(7)).as_deref(),
            Some(
                "fael: chunk U1 brief changed since you started (brief, scope) — read it: fael chunk show U1\n"
            )
        );
        assert_eq!(say(drift(7)), None);
        assert!(say(drift(9)).is_some(), "a later edit says again");
    }
}
