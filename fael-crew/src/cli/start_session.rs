//! The agent session a `chunk start` without `--run` binds to its run row:
//! explicit `--session` first, else the client env. Native ids only — an
//! RFC3339 hook timestamp is never stored; those runs stay alive via the
//! worktree-matched `seen()` last_seen-only path (the rule in
//! `fael-crew/src/agent.rs` stands). Split out of `chunk_run.rs` (file-size
//! ratchet); run-telemetry t2.

/// Resolve the start-time session: explicit `--session` (MCP `chunk start`,
/// hidden: the caller session when the MCP server's inherited env cannot name
/// it) first, else `FAEL_SESSION` first (the OpenCode shell.env hook sets it
/// per command, fresher than an inherited client var), `CLAUDE_CODE_SESSION_ID`,
/// `CODEX_THREAD_ID`. Ponytail: Claude Code's session env only; another
/// client's when it has one. `None` on a launcher run (`--run`) or when only a
/// hook timestamp is at hand — never stored, never guessed.
pub(crate) fn resolve_session<'a>(
    explicit: Option<String>,
    env: impl Fn(&'a str) -> Option<String>,
    no_run: bool,
) -> Option<String> {
    if !no_run {
        return None;
    }
    let native = |s: &str| !s.is_empty() && fael_core::ts_ms(s).is_none();
    explicit
        .map(|v| v.trim().to_string())
        .filter(|v| native(v))
        .or_else(|| {
            ["FAEL_SESSION", "CLAUDE_CODE_SESSION_ID", "CODEX_THREAD_ID"]
                .iter()
                .filter_map(|k| env(k))
                .map(|s| s.trim().to_string())
                .find(|s| native(s))
        })
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
            resolve_session(None, env(&[("FAEL_SESSION", stamp)]), true),
            None
        );
        assert_eq!(
            resolve_session(None, env(&[("CLAUDE_CODE_SESSION_ID", stamp)]), true),
            None
        );
        assert_eq!(
            resolve_session(Some(stamp.to_string()), env(&[]), true),
            None
        );
        assert_eq!(
            resolve_session(Some("  ".to_string()), env(&[]), true),
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
                true
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
                true
            ),
            Some("ses_fael_1".to_string())
        );
        assert_eq!(
            resolve_session(None, env(&[("CODEX_THREAD_ID", "thread_1")]), true),
            Some("thread_1".to_string())
        );
    }

    #[test]
    fn launcher_run_never_takes_a_session() {
        assert_eq!(
            resolve_session(
                Some("ses_x".to_string()),
                env(&[("FAEL_SESSION", "ses_fael_1")]),
                false
            ),
            None
        );
        assert_eq!(
            resolve_session(None, env(&[("FAEL_SESSION", "ses_fael_1")]), false),
            None
        );
    }
}
