//! A client that calls fael but whose hooks never fire. The wiring check reads
//! config files, not whether events arrive: Codex only runs `~/.codex/hooks.json`
//! after the user trusts it in `/hooks`, so a wired but untrusted client looks
//! fine to the file check. Usage knows better — calls from a client's sessions
//! against that client's hook events in the same window. Machine-wide on
//! purpose: the hook config is per client, not per repo.

use crate::core;
use std::collections::BTreeMap;

/// Threshold trial: 7 days, ≥ 20 calls and no hook event — a few calls can be
/// a session that simply ran before the hooks were wired.
const WINDOW_MS: i64 = 7 * 24 * 3600 * 1000;
const MIN_CALLS: usize = 20;
/// The clients that have hooks; `cli` and `mcp` calls have none to miss.
const CLIENTS: [&str; 3] = ["claude", "codex", "opencode"];

/// One note per hook client that made `MIN_CALLS`+ session calls in the window
/// and produced no hook event (any usage line that is not a `call`).
pub(super) fn notes() -> Vec<core::Problem> {
    let path = core::stats::state_dir().join("usage.jsonl");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    lines(&text, core::now_ms() as i64)
        .into_iter()
        .map(|l| core::Problem::info(core::ProblemKind::Wiring, l))
        .collect()
}

fn lines(text: &str, now: i64) -> Vec<String> {
    let mut seen: BTreeMap<&str, (usize, usize)> = BTreeMap::new(); // (calls, hook events)
    for l in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(l) else {
            continue;
        };
        let in_window = v["ts"]
            .as_str()
            .and_then(core::ts_ms)
            .is_some_and(|t| now - t <= WINDOW_MS);
        let Some(client) = v["client"]
            .as_str()
            .and_then(|c| CLIENTS.iter().find(|k| **k == c))
        else {
            continue;
        };
        if !in_window {
            continue;
        }
        let e = seen.entry(client).or_default();
        if v["event"] == "call" {
            e.0 += usize::from(v["session"].is_string());
        } else {
            e.1 += 1;
        }
    }
    seen.into_iter()
        .filter(|(_, (calls, events))| *calls >= MIN_CALLS && *events == 0)
        .map(|(c, (calls, _))| {
            let fix = if c == "codex" {
                "in Codex run /hooks and trust fael"
            } else {
                "`fael upgrade` rewrites the wiring; then restart the client"
            };
            format!("{c}: {calls} calls, 0 hook events — hooks are not firing; {fix}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(client: &str, n: usize) -> String {
        (0..n)
            .map(|_| format!("{{\"ts\":\"2026-11-10T00:00:00.000Z\",\"client\":\"{client}\",\"event\":\"call\",\"session\":\"s\"}}\n"))
            .collect()
    }

    #[test]
    fn silent_hooks_are_named_and_events_or_few_calls_stay_quiet() {
        let now = core::ts_ms("2026-11-11T00:00:00Z").unwrap();
        let out = lines(&call("codex", 100), now);
        assert_eq!(
            out,
            [
                "codex: 100 calls, 0 hook events — hooks are not firing; in Codex run /hooks and trust fael"
            ]
        );
        let ev = "{\"ts\":\"2026-11-10T00:00:00.000Z\",\"client\":\"codex\",\"event\":\"silent-start\"}\n";
        assert!(lines(&(call("codex", 100) + ev), now).is_empty());
        assert!(lines(&call("codex", 19), now).is_empty());
        assert!(lines(&call("cli", 100), now).is_empty());
        // 8 days old: outside the window
        assert!(lines(&call("codex", 100), now + 7 * 24 * 3600 * 1000).is_empty());
    }
}
