//! The inbox line (lane-force-receipt l5): a `fael chunk note` left for the
//! running chunk since its run started. The Overlap push tells A to
//! `fael chunk note` B, but B reads it only at its next start — this says it
//! on the next session start instead, and at the next edit. One line per push
//! for the chunk: one note is one line; several are a header, one bullet per
//! note and the reply and full-text commands once. Each note is said once per
//! session. Inform only: no edit is blocked. One read-only SQLite read, no
//! git spawn.

use super::protocol::Ctx;
use super::say::{Kind, Line, Outbox};
use crate::core::work;

/// Threshold trial: the inbox shows the note's first line only — a 2000-char
/// note is one line, the full text one `fael chunk show` away.
pub(crate) const INBOX_CHARS: usize = 120;

/// Notes listed in one push, the newest. Older unsaid ones are only counted:
/// twenty truncated notes cannot be acted on from a line, and `fael chunk
/// show` prints them all.
pub(crate) const INBOX_MAX: usize = 5;

fn short(text: &str) -> String {
    let single = text.replace('\n', " ");
    let cut: String = single.chars().take(INBOX_CHARS).collect();
    cut.trim().to_string()
}

/// The one line for the notes `(id, text)` not yet said, oldest first; `None`
/// with no note. The newest `INBOX_MAX` are listed, the rest counted, and
/// their keys are spent with the line (`Kind::Inbox::more`).
fn one(uid: &str, notes: &[(i64, String)]) -> Option<Line> {
    let (older, shown) = notes.split_at(notes.len().saturating_sub(INBOX_MAX));
    let tail = format!("reply: fael chunk note {uid} \"<text>\" · full: fael chunk show {uid}\n");
    let text = match shown {
        [] => return None,
        [(_, t)] if older.is_empty() => {
            format!("fael: new note for chunk {uid}: {} — {tail}", short(t))
        }
        _ => {
            let mut text = format!("fael: {} new notes for chunk {uid}", notes.len());
            if !older.is_empty() {
                text.push_str(&format!(" (newest {} listed)", shown.len()));
            }
            text.push_str(":\n");
            for (_, t) in shown {
                text.push_str(&format!("- {}\n", short(t)));
            }
            text.push_str(&tail);
            text
        }
    };
    let ids = |ns: &[(i64, String)]| ns.iter().map(|(id, _)| id.to_string()).collect();
    Some(Line {
        kind: Kind::Inbox {
            ids: ids(shown),
            more: ids(older),
            uid: uid.into(),
        },
        text,
    })
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
/// the reader's own and those `out` already said this session — as one line.
/// Empty with no crew.db, no live run or no new notes.
pub(crate) fn lines(c: &Ctx, out: &Outbox) -> Vec<Line> {
    let db = work::db_for(c.repo.journal.as_deref(), &c.repo.fael);
    let Some((uid, notes)) = work::inbox_notes(&db, &c.repo.root.to_string_lossy(), &c.session)
    else {
        return vec![];
    };
    let new: Vec<(i64, String)> = notes
        .into_iter()
        .filter(|(_, _, s)| !is_own(s.as_deref(), &c.session))
        .filter(|(id, _, _)| !out.has(&Kind::inbox_key(&uid, &id.to_string())))
        .map(|(id, t, _)| (id, t))
        .collect();
    one(&uid, &new).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::say::Outbox;
    use crate::hook::say_card::At;
    use crate::hook::state::lock_seen;

    #[test]
    fn one_line_per_note_said_once_per_note_id() {
        let l1 = one("U1", &[(3, "hello".into())]).unwrap();
        let l2 = one("U1", &[(4, "world".into())]).unwrap();
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
        let l = one("U1", &[(3, long.clone())]).unwrap();
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

    /// Several notes are one line: the reply and show commands once, one bullet
    /// per note, and past `INBOX_MAX` only the newest listed — every note's key
    /// spent with the line, the listed ones alone named in `said`.
    #[test]
    fn many_notes_share_one_header_and_one_tail() {
        let notes: Vec<(i64, String)> = (1..=7).map(|i| (i, format!("note number {i}"))).collect();
        let l = one("U1", &notes).unwrap();
        assert_eq!(
            l.text.matches("fael chunk note U1").count(),
            1,
            "{}",
            l.text
        );
        assert_eq!(
            l.text.matches("fael chunk show U1").count(),
            1,
            "{}",
            l.text
        );
        assert_eq!(l.text.lines().filter(|x| x.starts_with("- ")).count(), 5);
        assert!(
            l.text
                .starts_with("fael: 7 new notes for chunk U1 (newest 5 listed):")
        );
        assert!(!l.text.contains("note number 2") && l.text.contains("note number 3"));
        assert!(l.text.contains("note number 7"));
        assert_eq!(l.kind.keys().len(), 7, "older keys are spent too");
        let small = one("U1", &notes[..2]).unwrap();
        assert!(small.text.starts_with("fael: 2 new notes for chunk U1:\n"));
        assert!(one("U1", &[]).is_none());
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
