//! Per-session stash: the risk marker and fix phrase `stop` files for the
//! next push to show once. Keyed by (session, agent) — the parent thread keeps
//! the historical `<base>.risk`/`.fixed` paths, a sub-agent stashes at
//! `<base>.<agent>.risk`/`.fixed` so its line survives until the parent's next
//! push shows it. A sub-agent's own push never reads these.

use super::state::{session_id, session_key, state_dir};
use std::path::{Path, PathBuf};

/// A Weak risk line stashed by stop for the next push — shown once, deleted.
/// Stemmed like `edits_path` (decision 01M71AJMH4RSS13BZKZEWASQK8).
/// Keyed by (session, agent): the parent thread keeps the old
/// `<base>.risk` path, a sub-agent stashes at `<base>.<agent>.risk` so its
/// line survives until the parent's next push shows it.
pub(crate) fn risk_path(session: &str, agent: &str, root: &Path) -> PathBuf {
    let id = session_id(session);
    let base = session_key(&format!("{id}\0{}", root.to_string_lossy()));
    let name = match agent {
        "" => format!("{base}.risk"),
        a => format!("{base}.{}.risk", session_key(a)),
    };
    state_dir().join("sessions").join(name)
}

/// A fix phrase stashed by stop for the next push (PLAN-fael-experience-loop
/// chunk 5a) — shown once, deleted. Stemmed like `edits_path` (decision
/// 01M71AJMH4RSS13BZKZEWASQK8). Keyed by (session, agent) like `risk_path`.
pub(crate) fn fixed_path(session: &str, agent: &str, root: &Path) -> PathBuf {
    let id = session_id(session);
    let base = session_key(&format!("{id}\0{}", root.to_string_lossy()));
    let name = match agent {
        "" => format!("{base}.fixed"),
        a => format!("{base}.{}.fixed", session_key(a)),
    };
    state_dir().join("sessions").join(name)
}

/// The base key of a session + worktree: the parent stash file plus every
/// `<base>.<agent>.*` sub-agent stash share this prefix.
fn stash_base(session: &str, root: &Path) -> String {
    let id = session_id(session);
    session_key(&format!("{id}\0{}", root.to_string_lossy()))
}

/// The stashed risk note and fix phrase, if any — left
/// on disk until `clear_stash`, so a push that had no budget for them leaves
/// them for the next. `agent` names the thread that stashed: the parent's
/// own stash when empty, a sub-agent's when set.
pub(crate) fn peek_stash(session: &str, agent: &str, root: &Path) -> [Option<String>; 2] {
    let peek = |path: &Path| {
        let s = std::fs::read_to_string(path).ok()?;
        let s = s.trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    [risk_path, fixed_path].map(|p| peek(&p(session, agent, root)))
}

/// Every stashed risk marker and fix phrase for a session + worktree: the
/// parent's own stash first, then each sub-agent's, sorted for a stable push
/// order. A sub-agent's line survives its own push — `push.rs` never reads
/// these for it — until the parent's next push shows it.
pub(crate) fn peek_all_stash(session: &str, root: &Path) -> (Vec<String>, Vec<String>) {
    let mut risks = vec![];
    let mut fixeds = vec![];
    let [risk, fixed] = peek_stash(session, "", root);
    risks.extend(risk);
    fixeds.extend(fixed);
    let parent_risk = risk_path(session, "", root);
    let parent_fixed = fixed_path(session, "", root);
    let base = stash_base(session, root);
    let dir = state_dir().join("sessions");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (risks, fixeds);
    };
    let mut subs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            if p == &parent_risk || p == &parent_fixed {
                return false;
            }
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with(&base) && n.len() > base.len() && n[base.len()..].starts_with('.')
            })
        })
        .collect();
    subs.sort();
    for p in subs {
        let is_risk = p.extension().is_some_and(|e| e == "risk");
        let is_fixed = p.extension().is_some_and(|e| e == "fixed");
        if !is_risk && !is_fixed {
            continue;
        }
        let Some(s) = std::fs::read_to_string(&p)
            .ok()
            .map(|s| s.trim().to_string())
        else {
            continue;
        };
        if s.is_empty() {
            continue;
        }
        if is_risk {
            risks.push(s);
        } else {
            fixeds.push(s);
        }
    }
    (risks, fixeds)
}

/// Delete the stashes — once their line was said, never before. Clears the
/// parent's own stash plus every sub-agent's for the session, so a shown
/// sub-agent line is not repeated.
pub(crate) fn clear_stash(session: &str, root: &Path) {
    if session.is_empty() {
        return;
    }
    let base = stash_base(session, root);
    let dir = state_dir().join("sessions");
    for p in [risk_path(session, "", root), fixed_path(session, "", root)] {
        let _ = std::fs::remove_file(p);
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            let is_stash = p.extension().is_some_and(|e| e == "risk" || e == "fixed");
            if !is_stash {
                continue;
            }
            if p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with(&base) && n.len() > base.len() && n[base.len()..].starts_with('.')
            }) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::session_id;

    /// Stem-once (decision 01M71AJMH4RSS13BZKZEWASQK8): a transcript path
    /// and its bare stem address the same session files, and the parent
    /// thread keeps the historical path while a sub-agent gets its own suffix.
    #[test]
    fn stash_paths_agree_across_forms_and_split_per_agent() {
        let root = std::path::Path::new("/w");
        let (path, stem) = ("/sessions/abc123.jsonl", "abc123");
        assert_eq!(session_id(path), session_id(stem));
        assert_eq!(
            super::risk_path(path, "", root),
            super::risk_path(stem, "", root)
        );
        assert_eq!(
            super::fixed_path(path, "", root),
            super::fixed_path(stem, "", root)
        );
        assert_ne!(
            super::risk_path(stem, "", root),
            super::risk_path(stem, "a1", root)
        );
        assert_ne!(
            super::fixed_path(stem, "", root),
            super::fixed_path(stem, "a1", root)
        );
    }
}
