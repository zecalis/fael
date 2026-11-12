//! Split distinct `*:handoff` rows by writer → receiver client (pair-stat).
//! Same two resolutions as `cross.rs`: the writer from the row
//! (`extra.client`, falling back to its writer session's client) and the
//! receiver from the pushed session's first agent client. Pure.

use super::parse::{Parsed, session_stem};
use crate::{Log, Row};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Lines of the fael CLI and MCP server carry the caller's session but not
/// the agent's client (same rule as `cross.rs`).
fn is_agent(client: &str) -> bool {
    !matches!(client, "cli" | "mcp" | "neutral")
}

/// Session stem → first agent client, over kept + call lines.
fn session_clients(parsed: &Parsed) -> HashMap<&str, &str> {
    let mut clients: HashMap<&str, &str> = HashMap::new();
    for v in parsed.kept.iter().chain(parsed.calls.iter()) {
        if let (Some(s), Some(c)) = (
            v["session"].as_str(),
            v["client"].as_str().filter(|c| is_agent(c)),
        ) {
            clients.entry(session_stem(s)).or_insert(c);
        }
    }
    clients
}

/// Repo → its `*:handoff` row ids.
fn handoff_ids(logs: &HashMap<String, Log>) -> HashMap<&str, HashSet<&str>> {
    logs.iter()
        .map(|(repo, log)| {
            let h = log
                .rows
                .iter()
                .filter(|r| r.key.as_deref().is_some_and(|k| k.ends_with(":handoff")));
            (repo.as_str(), h.map(|r| r.id.as_str()).collect())
        })
        .collect()
}

/// Handoff id → its first push's receiver session, in file (= time) order.
fn first_receiver(parsed: &Parsed) -> HashMap<&str, &str> {
    let mut first: HashMap<&str, &str> = HashMap::new();
    for v in &parsed.kept {
        if let Some(session) = v["session"].as_str() {
            for id in v["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| i.as_str())
            {
                first.entry(id).or_insert(session);
            }
        }
    }
    first
}

/// Writer client of a handoff row, with the session fallback.
fn writer_client<'a>(row: &'a Row, clients: &HashMap<&str, &'a str>) -> Option<&'a str> {
    let row_client = row
        .extra
        .get("client")
        .and_then(|v| v.as_str())
        .filter(|c| is_agent(c));
    match row
        .extra
        .get("session")
        .and_then(|w| w.as_str())
        .map(session_stem)
    {
        Some(w) => row_client.or_else(|| clients.get(w).copied()),
        None => row_client,
    }
}

/// One row counts once, under its first push's receiver; either side unknown
/// lands in `writer_unknown` so the parts add up to the total. Never says the
/// receiver acted on it.
pub(super) fn handoff_split(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
) -> (usize, BTreeMap<String, usize>, usize, usize) {
    let clients = session_clients(parsed);
    let handoffs = handoff_ids(logs);
    let first = first_receiver(parsed);
    let by_id: HashMap<&str, HashMap<&str, &Row>> = logs
        .iter()
        .map(|(repo, log)| {
            (
                repo.as_str(),
                log.rows.iter().map(|r| (r.id.as_str(), r)).collect(),
            )
        })
        .collect();
    let mut by_client: BTreeMap<String, usize> = BTreeMap::new();
    let (mut same, mut unknown, mut total) = (0usize, 0usize, 0usize);
    for (id, repos) in &parsed.id_repos {
        let mut row_opt: Option<&Row> = None;
        for repo in repos {
            if handoffs
                .get(repo.as_str())
                .is_some_and(|h| h.contains(id.as_str()))
                && let Some(m) = by_id.get(repo.as_str())
                && let Some(r) = m.get(id.as_str())
            {
                row_opt = Some(r);
                break;
            }
        }
        let Some(row) = row_opt else { continue };
        total += 1;
        let receiver = first
            .get(id.as_str())
            .map(|s| session_stem(s))
            .and_then(|s| clients.get(s).copied());
        match (writer_client(row, &clients), receiver) {
            (Some(w), Some(r)) if w == r => same += 1,
            (Some(w), Some(r)) => {
                *by_client.entry(format!("{w}→{r}")).or_default() += 1;
            }
            _ => unknown += 1,
        }
    }
    (total, by_client, same, unknown)
}
