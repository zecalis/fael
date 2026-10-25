//! A note retires from the push once it was said in `SESSIONS` distinct
//! sessions (vela, 2026-10-10..25: 40 notes said in five or more sessions made
//! 519 of 682 note lines, and 27 of the 40 were spent hand-offs — "A2 done",
//! said in 68 sessions — that the chunk-done / merged-branch proof in
//! `retired.rs` caught 29 lines of, since a squash merge hides its proof).
//! fael never reads the note's text: five sessions that each met it at an edit
//! and left it open is the evidence. Only notes — a decision or an issue is
//! asked for by its own lifecycle. A bump keeps the note's id, so it does not
//! reset the count; a new version (`--supersedes`) has a new id and starts at
//! zero. The note stays in the log: `fael find --all` still finds it, only the
//! hook push skips it (this is hook-only state, not read by `find`).
//!
//! Per-machine state, never in `.fael/`: `<id>\t<session>,<session>,…` per
//! line, per clone (keyed by the clone-shared journal, like the log). Fails
//! open everywhere: an unreadable file is count 0 (the note is said as
//! before), an unwritable one drops the record.

use super::state::{session_key, state_dir};
use crate::core;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Distinct sessions a note is said in before the push drops it.
pub(crate) const SESSIONS: usize = 5;

/// Backstop on top of the closed/superseded prune: the newest ids win.
const MAX_ENTRIES: usize = 1000;

/// note id -> the sessions (at most `SESSIONS`) that were told it
pub(crate) struct Count(BTreeMap<String, Vec<String>>);

fn path(share: &Path) -> PathBuf {
    let key = session_key(&share.to_string_lossy());
    state_dir().join("spent").join(format!("{key}.tsv"))
}

impl Count {
    /// One file read. A bad line is skipped.
    pub(crate) fn load(share: &Path) -> Count {
        let s = std::fs::read_to_string(path(share)).unwrap_or_default();
        Count(
            s.lines()
                .filter_map(|l| l.split_once('\t'))
                .map(|(id, ss)| (id.into(), ss.split(',').map(String::from).collect()))
                .collect(),
        )
    }

    /// Other sessions that were told `id`.
    fn others(&self, id: &str, session: &str) -> usize {
        self.0
            .get(id)
            .map_or(0, |v| v.iter().filter(|s| *s != session).count())
    }

    /// Notes already said in `SESSIONS` sessions other than this one.
    pub(crate) fn retired(&self, session: &str) -> HashSet<String> {
        let spent = |id: &&String| self.others(id, session) >= SESSIONS;
        self.0.keys().filter(spent).cloned().collect()
    }

    /// Of the notes about to be said, those this session is the last to be
    /// told: it makes the `SESSIONS`th distinct one.
    pub(crate) fn crossing<'a>(&self, session: &str, ids: &[&'a str]) -> Vec<&'a str> {
        let last = |id: &&str| self.others(id, session) == SESSIONS - 1;
        let told = |id: &&str| {
            self.0
                .get(*id)
                .is_some_and(|v| v.iter().any(|s| s == session))
        };
        ids.iter()
            .copied()
            .filter(|i| last(i) && !told(i))
            .collect()
    }
}

/// Remember `session` against the notes just said. Writes only when a count
/// moved; entries of closed or superseded notes are dropped, then the newest
/// `MAX_ENTRIES` stay.
pub(crate) fn record(log: &core::Log, share: &Path, session: &str, said: &[&str]) {
    if said.is_empty() || session.is_empty() {
        return;
    }
    let mut c = Count::load(share);
    let mut moved = false;
    for id in said {
        let v = c.0.entry(id.to_string()).or_default();
        if v.len() < SESSIONS && !v.iter().any(|s| s == session) {
            v.push(session.into());
            moved = true;
        }
    }
    if !moved {
        return;
    }
    let (closed, gone) = (core::closed(log), core::superseded(log));
    c.0.retain(|id, _| !closed.contains(id.as_str()) && !gone.contains(id.as_str()));
    // ids are time-ordered, so the largest are the newest
    let body: String =
        c.0.iter()
            .rev()
            .take(MAX_ENTRIES)
            .map(|(id, v)| format!("{id}\t{}\n", v.join(",")))
            .collect();
    let p = path(share);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(tmp, p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A share dir of its own, so tests never meet.
    fn share() -> PathBuf {
        std::env::temp_dir().join(format!("fael-spent-{}", core::ulid()))
    }

    fn told(share: &Path, id: &str, n: usize) {
        for i in 0..n {
            record(&core::Log::default(), share, &format!("s{i}"), &[id]);
        }
    }

    #[test]
    fn a_note_retires_after_five_other_sessions_never_before() {
        let sh = share();
        told(&sh, "N1", 4);
        assert!(Count::load(&sh).retired("s9").is_empty());
        told(&sh, "N1", 5);
        assert!(Count::load(&sh).retired("s9").contains("N1"));
        // the fifth session itself still sees it: four others
        assert!(Count::load(&sh).retired("s0").is_empty());
    }

    #[test]
    fn the_crossing_session_is_the_fifth_distinct_one_and_only_once() {
        let sh = share();
        told(&sh, "N1", 4);
        assert_eq!(Count::load(&sh).crossing("s4", &["N1", "N2"]), vec!["N1"]);
        record(&core::Log::default(), &sh, "s4", &["N1"]);
        // said again in the same session, or by a sixth: not a crossing
        assert!(Count::load(&sh).crossing("s4", &["N1"]).is_empty());
        assert!(Count::load(&sh).crossing("s5", &["N1"]).is_empty());
    }

    #[test]
    fn one_session_counts_once_and_a_sixth_is_not_kept() {
        let sh = share();
        told(&sh, "N1", 3);
        told(&sh, "N1", 3);
        assert_eq!(Count::load(&sh).0["N1"].len(), 3);
        told(&sh, "N1", 5);
        record(&core::Log::default(), &sh, "s7", &["N1"]);
        assert_eq!(Count::load(&sh).0["N1"].len(), SESSIONS);
    }

    #[test]
    fn closed_notes_are_pruned_and_the_file_is_capped() {
        let sh = share();
        told(&sh, "N1", 1);
        let mut log = core::Log::default();
        log.closes.push(core::Row {
            id: "C1".into(),
            reference: Some("N1".into()),
            ..core::Row::default()
        });
        record(&log, &sh, "s0", &["N2"]);
        assert!(!Count::load(&sh).0.contains_key("N1"));
        for i in 0..MAX_ENTRIES + 5 {
            record(&core::Log::default(), &sh, "s0", &[&format!("N{i:05}")]);
        }
        let c = Count::load(&sh);
        assert_eq!(c.0.len(), MAX_ENTRIES);
        assert!(c.0.contains_key(&format!("N{:05}", MAX_ENTRIES + 4)));
    }

    #[test]
    fn unreadable_or_unwritable_state_fails_open() {
        let sh = share();
        assert!(Count::load(&sh).retired("s").is_empty());
        let p = path(&sh);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "garbage no tab\n\u{0}\n").unwrap();
        assert!(Count::load(&sh).retired("s").is_empty());
        // the tmp path is a directory: the write fails and nothing panics
        std::fs::create_dir_all(p.with_extension("tmp")).unwrap();
        told(&sh, "N1", 2);
    }
}
