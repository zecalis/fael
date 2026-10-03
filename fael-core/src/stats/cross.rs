//! What fael did across parallel agents: rows one session wrote and another
//! was handed (PLAN stats:cross-agent). Deterministic joins only — a pushed
//! (session, row) pair against the writer session the row carries. Pure:
//! parsed usage and loaded logs in.

use super::parse::Parsed;
use crate::{Log, Row, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Pairs of one kind, and how many were still in context at an edit.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Reuse {
    pub pushed: usize,
    pub in_context_at_edit: usize,
}

/// Only rows that name their writer session count (`fael add` inside a hook
/// session tags it; older rows and rows filed outside a session do not), over
/// the same window as `by_event`. Each field is a subset of the one above it
/// in meaning, not in count: a pair can be in both of the last two.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CrossAgent {
    /// The row was written by another session than the one handed it.
    pub other_session: Reuse,
    /// ... and that session ran in another worktree (its usage lines name a
    /// repo path the receiver is not in; a writer with no usage is unknown).
    pub other_worktree: Reuse,
    /// ... and was written after the receiving session's first usage line:
    /// two agents working at once, one's row reaching the other.
    pub written_during_session: Reuse,
    /// ... and the two sessions ran in different clients (Claude Code, OpenCode,
    /// Codex …). Needs both clients known from their usage lines.
    pub other_client: Reuse,
    /// `other_client` split by `"<writer client>→<receiver client>"`.
    pub by_client: BTreeMap<String, Reuse>,
    /// Pushed pairs whose row names no writer session, so none of the above
    /// can place them: rows from clients that give `fael add` no session id,
    /// and rows older than the tag.
    pub writer_unknown: usize,
}

/// Usage lines name a Claude session by its transcript path, rows by that
/// file's stem; other clients use one string for both.
fn stem(s: &str) -> &str {
    let f = s.rsplit(['/', '\\']).next().unwrap_or(s);
    f.strip_suffix(".jsonl").unwrap_or(f)
}

/// Lines of the fael CLI and MCP server carry the caller's session but not
/// the agent's client.
fn is_agent(client: &str) -> bool {
    !matches!(client, "cli" | "mcp" | "neutral")
}

type Key<'a> = (&'a str, &'a str, &'a str);

pub(super) fn cross_agent(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    pushed: &HashMap<Key<'_>, &str>,
    in_context: &HashSet<Key<'_>>,
) -> CrossAgent {
    // session → first usage ms, and the worktrees it ran in
    let mut first: HashMap<&str, i64> = HashMap::new();
    let mut repos: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut clients: HashMap<&str, &str> = HashMap::new();
    for v in &parsed.kept {
        let (Some(s), Some(r)) = (v["session"].as_str(), v["repo"].as_str()) else {
            continue;
        };
        let s = stem(s);
        if let Some(c) = v["client"].as_str().filter(|c| is_agent(c)) {
            clients.entry(s).or_insert(c);
        }
        repos.entry(s).or_default().insert(r);
        if let Some(ms) = v["ts"].as_str().and_then(ts_ms) {
            first
                .entry(s)
                .and_modify(|f| *f = (*f).min(ms))
                .or_insert(ms);
        }
    }
    let by_id: HashMap<&str, HashMap<&str, &Row>> = logs
        .iter()
        .map(|(repo, log)| {
            (
                repo.as_str(),
                log.rows.iter().map(|r| (r.id.as_str(), r)).collect(),
            )
        })
        .collect();
    let mut out = CrossAgent::default();
    for key @ (repo, session, id) in pushed.keys() {
        let session = stem(session);
        let Some(row) = by_id.get(repo).and_then(|m| m.get(id)) else {
            continue;
        };
        let Some(writer) = row.extra.get("session").and_then(|w| w.as_str()) else {
            out.writer_unknown += 1;
            continue;
        };
        let writer = stem(writer);
        if writer == session {
            continue;
        }
        // the set is keyed by the raw usage session, not the stem
        let hit = usize::from(in_context.contains(key));
        let add = |r: &mut Reuse| {
            r.pushed += 1;
            r.in_context_at_edit += hit;
        };
        add(&mut out.other_session);
        if repos.get(writer).is_some_and(|w| !w.contains(repo)) {
            add(&mut out.other_worktree);
        }
        let started = first.get(session);
        if ts_ms(&row.ts).zip(started).is_some_and(|(w, s)| w >= *s) {
            add(&mut out.written_during_session);
        }
        if let (Some(w), Some(r)) = (clients.get(writer), clients.get(session))
            && w != r
        {
            add(&mut out.other_client);
            add(out.by_client.entry(format!("{w}→{r}")).or_default());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::Log;
    use std::collections::HashMap;

    fn row(id: &str, ts: &str, extra: &str) -> String {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"{ts}\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]{extra}}}\n"
        )
    }

    fn line(
        ts: &str,
        repo: &str,
        client: &str,
        event: &str,
        session: &str,
        ids: &str,
        ctx: &str,
    ) -> String {
        format!(
            "{{\"ts\":\"{ts}\",\"repo\":\"{repo}\",\"client\":\"{client}\",\"event\":\"{event}\",\"bytes\":9,\"est_tokens\":2,\"ids\":[{ids}],\"in_context\":[{ctx}],\"session\":\"{session}\"}}\n"
        )
    }

    #[test]
    fn counts_rows_that_crossed_sessions_worktrees_and_clients() {
        let (mut rows, mut w) = (vec![], vec![]);
        let text = row("R1", "2026-09-26T00:00:30.000Z", ",\"session\":\"w1\"")
            // before the receiver started, from a writer with no usage lines
            + &row("R2", "2026-09-24T00:00:00.000Z", ",\"session\":\"w2\"")
            + &row("R3", "2026-09-26T00:00:30.000Z", "")
            // the receiver's own row, pushed back to it: not another agent
            + &row("R4", "2026-09-26T00:00:40.000Z", ",\"session\":\"r1\"");
        crate::log::parse(text.as_bytes(), "t.jsonl", &mut rows, &mut w);
        let log = Log {
            rows,
            ..Default::default()
        };
        // r1 (Claude: usage names its transcript path, the row only the stem)
        // in /work/a is pushed R1..R4; w1 is an OpenCode session in /work/b.
        // R1 was written 30 s after r1 started and sits in context at an
        // edit. s0 is the feature's first in-context line, so every push is
        // in the window.
        let r1 = "/p/r1.jsonl";
        let (a, b) = ("/work/a", "/work/b");
        let usage = line("2026-09-25T00:00:00.000Z", a, "claude", "in-context", "s0", "", "\"Z9\"")
            + &line("2026-09-26T00:00:00.000Z", a, "claude", "session-start", r1, "", "")
            + &line("2026-09-26T00:00:20.000Z", b, "opencode", "session-start", "w1", "", "")
            // the CLI line of the same session names no agent client
            + &line("2026-09-26T00:00:25.000Z", b, "cli", "add", "w1", "", "")
            + &line("2026-09-26T00:01:00.000Z", a, "claude", "read", r1, "\"R1\",\"R2\",\"R3\",\"R4\"", "")
            + &line("2026-09-26T00:02:00.000Z", a, "claude", "in-context", r1, "", "\"R1\"");
        let parsed =
            super::super::parse::parse(&usage, std::path::Path::new("/s/usage.jsonl"), &[]);
        let logs = HashMap::from([("/work/a".to_string(), log)]);
        let v = super::super::value::value(&parsed, &logs);
        let c = v.cross_agent;
        // R1, R2 came from other sessions (R3 names none, R4 is its own)
        assert_eq!(
            (c.other_session.pushed, c.other_session.in_context_at_edit),
            (2, 1)
        );
        // only w1 has usage lines elsewhere; w2's worktree is unknown
        assert_eq!(
            (c.other_worktree.pushed, c.other_worktree.in_context_at_edit),
            (1, 1)
        );
        // R2 was written before r1 started
        assert_eq!(
            (
                c.written_during_session.pushed,
                c.written_during_session.in_context_at_edit
            ),
            (1, 1)
        );
        // R1 went from OpenCode to Claude; w2's client is unknown
        assert_eq!(
            (c.other_client.pushed, c.other_client.in_context_at_edit),
            (1, 1)
        );
        assert_eq!(c.by_client["opencode→claude"].pushed, 1);
        // R3 names no writer
        assert_eq!(c.writer_unknown, 1);
    }
}
