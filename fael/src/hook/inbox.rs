//! The inbox line (lane-force-receipt l5): a `fael chunk note` left for the
//! running chunk since its run started. The Overlap push tells A to
//! `fael chunk note` B, but B reads it only at its next start — this says it
//! on the next session start instead (SessionStart-only by design). One line per note event, said
//! once per note id. Inform only: no edit is blocked. One read-only SQLite
//! read, no git spawn.

use super::protocol::Ctx;
use super::say::{Kind, Line};
use crate::core::work;

/// Threshold trial: the inbox shows the note's first line only — a 2000-char
/// note is one line, the full text one `fael chunk show` away.
pub(crate) const INBOX_CHARS: usize = 120;

fn one(uid: &str, id: i64, text: &str) -> Line {
    let single = text.replace('\n', " ");
    let short: String = single.chars().take(INBOX_CHARS).collect();
    let short = short.trim();
    Line {
        kind: Kind::Inbox {
            id: id.to_string(),
            uid: uid.into(),
        },
        text: format!(
            "fael: new note for chunk {uid}: {short} — reply: fael chunk note {uid} \"<text>\" · full: fael chunk show {uid}\n"
        ),
    }
}

/// True when the note's writer is the reader: its session stems to the hook
/// session (a transcript path and its stem are the same session, an RFC3339
/// opencode session is itself). Old notes with no session still show.
pub(crate) fn is_own(writer: Option<&str>, reader: &str) -> bool {
    let Some(w) = writer.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let r = reader.trim();
    if r.is_empty() {
        return false;
    }
    super::state::session_id(w) == super::state::session_id(r)
}

/// The notes left for this session's live chunk since its run started, minus
/// the reader's own notes. Empty with no crew.db, no live run or no new notes.
pub(crate) fn lines(c: &Ctx) -> Vec<Line> {
    let db = work::db_for(c.repo.journal.as_deref(), &c.repo.fael);
    let Some((uid, notes)) = work::inbox_notes(&db, &c.repo.root.to_string_lossy(), &c.session)
    else {
        return vec![];
    };
    notes
        .into_iter()
        .filter(|(_, _, s)| !is_own(s.as_deref(), &c.session))
        .map(|(id, t, _)| one(&uid, id, &t))
        .collect()
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
        assert!(l1.text.contains("fael chunk show U1"));
        assert!(l1.text.contains("hello"));
        assert_eq!(l1.text.lines().count(), 1, "one line: {}", l1.text);
        assert!(
            l1.text.trim_end().ends_with("fael chunk show U1"),
            "ends in show: {}",
            l1.text
        );
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
    fn long_notes_truncate_to_one_line() {
        let long = "A1".repeat(1000);
        let l = one("U1", 3, &long);
        assert_eq!(l.text.lines().count(), 1);
        assert!(l.text.contains("fael chunk note U1"));
        assert!(l.text.contains("fael chunk show U1"));
        assert!(
            l.text.trim_end().ends_with("fael chunk show U1"),
            "{}",
            l.text
        );
        // the note body is cut: the full 2000 chars never reach context
        assert!(!l.text.contains(&long[..200]), "not inlined");
        assert!(l.text.len() < long.len(), "truncated");
    }

    #[test]
    fn own_notes_stay_silent() {
        assert!(is_own(Some("s1"), "s1"));
        assert!(is_own(Some("abc123"), "/tmp/sessions/abc123.jsonl"));
        assert!(!is_own(Some("other"), "s1"));
        assert!(!is_own(None, "s1"));
        assert!(!is_own(Some("s1"), ""));
    }

    #[test]
    fn no_crew_db_is_silent() {
        let none = std::env::temp_dir().join(format!("fael-inbox-{}", crate::core::ulid()));
        let db = none.join("crew.db");
        assert!(work::inbox_notes(&db, "/w", "").is_none());
    }
}
