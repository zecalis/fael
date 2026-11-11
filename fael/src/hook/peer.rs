//! The same-file line when no chunk runs: another session edited this
//! repo-relative file within the window, in this or a sibling worktree of the
//! same clone. Inform only: no edit is blocked. One line per file, said once
//! per file and session, with the ready `fael add note`.
//!
//! The edit lists live in `<state>/sessions/edits/*.jsonl` — per machine, not
//! per repo (`state.rs::edits_path`, keyed by session + worktree). Each line
//! carries its worktree, so this scans them all and keeps the ones whose
//! `main_of(worktree)` (same `gitdir:` read as the journal, raw when gone)
//! matches this checkout's. No git spawn.
//!
//! Old lines without a worktree are skipped: without it no clone can be
//! proved. Lines without a branch are skipped too — the line names it.

use super::protocol::Ctx;
use super::say::{Kind, Line};
use super::state::{session_id, state_dir};
use std::collections::HashMap;
use std::path::Path;

/// One parsed edit entry with the peer line's fields.
struct Entry {
    path: String,
    at: i64,
    worktree: String,
    session: String,
    client: String,
    branch: String,
}

fn parse_entry(v: &serde_json::Value) -> Option<Entry> {
    Some(Entry {
        path: v["path"].as_str()?.to_string(),
        at: crate::core::ts_ms(v["at"].as_str()?)?,
        worktree: v["worktree"].as_str()?.to_string(),
        session: v["session"].as_str()?.to_string(),
        client: v["client"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("agent")
            .to_string(),
        branch: v["branch"].as_str().filter(|s| !s.is_empty())?.to_string(),
    })
}

/// The other sessions' latest edit per file of `files`, within the window.
fn others(root: &Path, session: &str, files: &[String], now: i64) -> HashMap<String, Entry> {
    let window = crate::core::stats::OVERLAP_WINDOW_MS;
    let mine = crate::journal::main_of(root);
    let me = session_id(session).to_string();
    let mut out: HashMap<String, Entry> = HashMap::new();
    let dir = state_dir().join("sessions").join("edits");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        // a file untouched for 15 min holds nothing in the 10 min window
        if e.metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d.as_secs() > 15 * 60)
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(en) = parse_entry(&v) else {
                continue;
            };
            if !files.contains(&en.path) {
                continue;
            }
            if session_id(&en.session) == me {
                continue;
            }
            let age = now - en.at;
            if age < 0 || age > window {
                continue;
            }
            if crate::journal::main_of(Path::new(&en.worktree)) != mine {
                continue;
            }
            let keep = out.get(&en.path).is_none_or(|old: &Entry| en.at > old.at);
            if keep {
                out.insert(en.path.clone(), en);
            }
        }
    }
    out
}

fn text(file: &str, en: &Entry, now: i64) -> String {
    let mins = ((now - en.at).max(0) / 60_000).max(1);
    format!(
        "fael: {file} also edited by {} on {} {mins}m ago — leave them a word: fael add note \"…\" --files {file}\n",
        en.client, en.branch,
    )
}

/// One `Peer` line per edited file another session touched within the window.
/// Empty with no session, no state dir or no other edit. The chunk-overlap
/// hold (same file already covered) is applied by the caller, which knows
/// those lines.
pub(crate) fn lines(c: &Ctx, files: &[String]) -> Vec<Line> {
    if files.is_empty() || c.session.is_empty() {
        return vec![];
    }
    let now = crate::core::now_ms() as i64;
    others(&c.repo.root, &c.session, files, now)
        .into_iter()
        .map(|(file, en)| {
            let t = text(&file, &en, now);
            Line {
                kind: Kind::Peer { file },
                text: t,
            }
        })
        .collect()
}

/// Overlap plus peer: chunk overlap lines, then peer lines for files no
/// overlap already covers (a chunk line wins the file).
pub(crate) fn overlap_and_peer(c: &Ctx, files: &[String]) -> Vec<Line> {
    let overlap_lines = super::overlap::lines(c, files);
    let mut over = std::collections::HashSet::new();
    for l in &overlap_lines {
        if let super::say::Kind::Overlap { file, .. } = &l.kind {
            over.insert(file.clone());
        }
    }
    let mut out = overlap_lines;
    for l in lines(c, files) {
        let keep = match &l.kind {
            Kind::Peer { file } => !over.contains(file),
            _ => true,
        };
        if keep {
            out.push(l);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, at: i64, worktree: &str, session: &str) -> serde_json::Value {
        serde_json::json!({"path": path, "at": crate::core::rfc3339(at as u64), "worktree": worktree, "session": session, "client": "claude", "branch": "feat-x"})
    }

    fn scan(
        entries: Vec<serde_json::Value>,
        root: &Path,
        session: &str,
        files: &[String],
        now: i64,
    ) -> HashMap<String, Entry> {
        // same filter as `others`, over given entries (no fs)
        let window = crate::core::stats::OVERLAP_WINDOW_MS;
        let mine = crate::journal::main_of(root);
        let me = session_id(session).to_string();
        let mut out = HashMap::new();
        for v in &entries {
            let Some(en) = parse_entry(v) else {
                continue;
            };
            if !files.contains(&en.path) {
                continue;
            }
            if session_id(&en.session) == me {
                continue;
            }
            let age = now - en.at;
            if age < 0 || age > window {
                continue;
            }
            if crate::journal::main_of(Path::new(&en.worktree)) != mine {
                continue;
            }
            if out.get(&en.path).is_none_or(|old: &Entry| en.at > old.at) {
                out.insert(en.path.clone(), en);
            }
        }
        out
    }

    #[test]
    fn own_session_and_other_files_stay_silent() {
        let now = crate::core::now_ms() as i64;
        let es = vec![
            entry("src/a.rs", now - 2 * 60_000, "/w", "me"),
            entry("src/b.rs", now - 2 * 60_000, "/w", "other"),
        ];
        let got = scan(es, Path::new("/w"), "me", &["src/a.rs".into()], now);
        assert!(got.is_empty());
    }

    #[test]
    fn old_lines_without_branch_stay_silent() {
        let now = crate::core::now_ms() as i64;
        let v = serde_json::json!({"path": "src/a.rs", "at": crate::core::rfc3339((now - 60_000) as u64), "worktree": "/w", "session": "other"});
        assert!(parse_entry(&v).is_none());
    }

    #[test]
    fn line_names_client_branch_and_minutes() {
        let now = crate::core::now_ms() as i64;
        let en = Entry {
            path: "src/a.rs".into(),
            at: now - 2 * 60_000,
            worktree: "/w".into(),
            session: "other".into(),
            client: "claude".into(),
            branch: "feat-x".into(),
        };
        let t = text("src/a.rs", &en, now);
        assert!(
            t.contains("src/a.rs also edited by claude on feat-x 2m ago"),
            "{t}"
        );
        assert!(t.contains("fael add note"), "{t}");
        assert!(t.contains("--files src/a.rs"), "{t}");
    }
}
