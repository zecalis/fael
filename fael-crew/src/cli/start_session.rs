//! The agent session a `chunk start` without `--run` binds to its run row:
//! explicit `--session` first, else the client env. Native ids only — an
//! RFC3339 hook timestamp is never stored; those runs stay alive via the
//! worktree-matched `seen()` last_seen-only path (the rule in
//! `fael-crew/src/agent.rs` stands). Split out of `chunk_run.rs` (file-size
//! ratchet); run-telemetry t2.

use std::path::{Path, PathBuf};

fn session_key(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn session_id(s: &str) -> &str {
    if !s.contains(['/', '\\']) {
        return s;
    }
    Path::new(s)
        .file_stem()
        .and_then(|f| f.to_str())
        .unwrap_or(s)
}

fn edits_path(session: &str, root: &Path) -> PathBuf {
    let id = session_id(session);
    let key = session_key(&format!("{id}\0{}", root.to_string_lossy()));
    fael_core::stats::state_dir()
        .join("sessions")
        .join("edits")
        .join(format!("{key}.jsonl"))
}

fn started_path(session: &str, root: &Path) -> PathBuf {
    let id = session_id(session);
    let key = session_key(&format!("{id}\0{}", root.to_string_lossy()));
    fael_core::stats::state_dir()
        .join("sessions")
        .join(format!("{key}.started"))
}

fn record_time(root: &Path, id: &str) -> Option<std::time::SystemTime> {
    let m = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    [m(edits_path(id, root)), m(started_path(id, root))]
        .into_iter()
        .flatten()
        .max()
}

/// Resolve the start-time session: explicit `--session` first, else
/// `FAEL_SESSION` first, else Claude/Codex newest hook record in this
/// worktree wins, else today's order. Native ids only — RFC3339 never stored.
pub(crate) fn resolve_session<'a>(
    explicit: Option<String>,
    env: impl Fn(&'a str) -> Option<String>,
    no_run: bool,
    root: Option<&Path>,
) -> Option<String> {
    if !no_run {
        return None;
    }
    let native = |s: &str| !s.is_empty() && fael_core::ts_ms(s).is_none();
    if let Some(v) = explicit.map(|v| v.trim().to_string()).filter(|v| native(v)) {
        return Some(v);
    }
    let fael = env("FAEL_SESSION")
        .map(|s| s.trim().to_string())
        .filter(|s| native(s));
    let claude = env("CLAUDE_CODE_SESSION_ID")
        .map(|s| s.trim().to_string())
        .filter(|s| native(s));
    let codex = env("CODEX_THREAD_ID")
        .map(|s| s.trim().to_string())
        .filter(|s| native(s));
    if fael.is_some() {
        return fael;
    }
    if let (Some(c), Some(x), Some(r)) = (claude.clone(), codex.clone(), root) {
        match (record_time(r, &c), record_time(r, &x)) {
            (Some(a), Some(b)) if a != b => return Some(if b > a { x } else { c }),
            (None, Some(_)) => return Some(x),
            (Some(_), None) => return Some(c),
            _ => {}
        }
    }
    claude.or(codex)
}

#[cfg(test)]
mod tests {
    use super::resolve_session;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&'a str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn hook_timestamps_never_stored_explicit_or_env() {
        // the leak t2 closes: the env fallback stored hook timestamps
        let stamp = "2026-10-24T10:36:37.463Z";
        assert_eq!(
            resolve_session(None, env(&[("FAEL_SESSION", stamp)]), true, None),
            None
        );
        assert_eq!(
            resolve_session(None, env(&[("CLAUDE_CODE_SESSION_ID", stamp)]), true, None),
            None
        );
        assert_eq!(
            resolve_session(Some(stamp.to_string()), env(&[]), true, None),
            None
        );
        assert_eq!(
            resolve_session(Some("  ".to_string()), env(&[]), true, None),
            None
        );
    }

    #[test]
    fn native_session_explicit_first_then_env_order() {
        assert_eq!(
            resolve_session(
                Some("ses_x".to_string()),
                env(&[
                    ("FAEL_SESSION", "ses_fael_1"),
                    ("CLAUDE_CODE_SESSION_ID", "ses_claude_1"),
                ]),
                true,
                None
            ),
            Some("ses_x".to_string())
        );
        assert_eq!(
            resolve_session(
                None,
                env(&[
                    ("FAEL_SESSION", "ses_fael_1"),
                    ("CLAUDE_CODE_SESSION_ID", "ses_claude_1"),
                ]),
                true,
                None
            ),
            Some("ses_fael_1".to_string())
        );
        assert_eq!(
            resolve_session(None, env(&[("CODEX_THREAD_ID", "thread_1")]), true, None),
            Some("thread_1".to_string())
        );
    }

    #[test]
    fn launcher_run_never_takes_a_session() {
        assert_eq!(
            resolve_session(
                Some("ses_x".to_string()),
                env(&[("FAEL_SESSION", "ses_fael_1")]),
                false,
                None
            ),
            None
        );
        assert_eq!(
            resolve_session(None, env(&[("FAEL_SESSION", "ses_fael_1")]), false, None),
            None
        );
    }
}
