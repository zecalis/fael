//! The inbox line (lane-force-receipt l5): a `fael chunk note` left for the
//! running chunk since its run started. The Overlap push tells A to
//! `fael chunk note` B, but B reads it only at its next start — this says it
//! on the next edit and session start instead. One line per note event, said
//! once per note id. Inform only: no edit is blocked. One read-only SQLite
//! read, no git spawn.

use super::protocol::Ctx;
use super::say::{Kind, Line};
use crate::core::work;

fn one(uid: &str, id: i64, text: &str) -> Line {
    let single = text.replace('\n', " ");
    Line {
        kind: Kind::Inbox {
            id: id.to_string(),
            uid: uid.into(),
        },
        text: format!(
            "fael: new note for chunk {uid}: {single} — reply: fael chunk note {uid} \"<text>\" (said once per note)\n"
        ),
    }
}

/// The notes left for this session's live chunk since its run started.
/// Empty with no crew.db, no live run or no new notes.
pub(crate) fn lines(c: &Ctx) -> Vec<Line> {
    let db = work::db_for(c.repo.journal.as_deref(), &c.repo.fael);
    let Some((uid, notes)) = work::inbox_notes(&db, &c.repo.root.to_string_lossy(), &c.session)
    else {
        return vec![];
    };
    notes.into_iter().map(|(id, t)| one(&uid, id, &t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::say::Outbox;
    use crate::hook::say_card::At;
    use crate::hook::state::lock_seen;

    #[test]
    fn one_line_per_note_said_once_per_note_id() {
        let l1 = one("U1", 3, "hello");
        let l2 = one("U1", 4, "world");
        assert!(l1.text.contains("fael chunk note U1"));
        assert!(l1.text.contains("hello"));
        let seen = crate::hook::say_contract::seen("s.seen");
        let say = |l: Line| {
            let mut out = Outbox::open(At::SessionStart, lock_seen(&seen));
            out.say(l);
            out.reply().context().map(String::from)
        };
        assert!(say(l1.clone()).is_some());
        assert_eq!(say(l1), None, "same note id stays silent");
        assert!(say(l2).is_some(), "another note id still says");
        let _ = std::fs::remove_dir_all(seen.parent().unwrap());
    }

    #[test]
    fn no_crew_db_is_silent() {
        let none = std::env::temp_dir().join(format!("fael-inbox-{}", crate::core::ulid()));
        let db = none.join("crew.db");
        assert!(work::inbox_notes(&db, "/w", "").is_none());
    }
}
