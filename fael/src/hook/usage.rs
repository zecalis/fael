//! Usage accounting (SPEC §8): every injection into context, per machine —
//! never in git. `fael stats` reads it back.

use super::asks::{self, UsageMeta};
use super::askstats;
use super::state::{now_rfc3339, state_dir};
use crate::core;
use std::collections::HashMap;
use std::path::Path;

/// Every injection into context, per machine — never in git. Fails open:
/// a usage write never fails the command it rode along with. Asks (reject /
/// stop-block / warning), the session, and real transcript tokens ride in
/// `meta` — absent keys stay absent, so old readers keep working.
pub(crate) fn record_usage(
    client: &str,
    event: &str,
    repo: &Path,
    text: &str,
    ids: &[String],
    meta: &UsageMeta,
) {
    let mut row = serde_json::json!({
        "ts": now_rfc3339().unwrap_or_default(),
        "repo": repo.to_string_lossy(),
        "client": client,
        "event": event,
        "bytes": text.len(),
        "est_tokens": core::est_tokens(text),
        "ids": ids,
    });
    if let Some(ask) = meta.ask {
        row["ask"] = ask.into();
    }
    if let Some(session) = meta.session {
        row["session"] = session.into();
    }
    if let Some(real) = meta.real
        && let Ok(t) = serde_json::to_value(real)
    {
        row["real_tokens"] = t;
    }
    asks::append_row(row);
}

/// The log a stats row's repo has now — the tree + journal union the hooks
/// read, so a `store = "local"` repo (journal only) is not read as empty.
/// A repo path that no longer resolves falls back to its tree.
fn repo_log(repo: &str) -> core::Log {
    crate::repo_at(Path::new(repo))
        .map(|r| crate::read(&r))
        .unwrap_or_else(|_| core::read(&Path::new(repo).join(".fael")))
}

#[expect(
    clippy::too_many_lines,
    reason = "predates the lint — split, then drop"
)]
pub fn stats(json: bool, rows: bool) -> Result<(), String> {
    let path = state_dir().join("usage.jsonl");
    let s = std::fs::read_to_string(&path).unwrap_or_default();
    // without `--json` an empty log is one friendly line, not a table of zeros
    if s.trim().is_empty() && !json {
        println!("fael: no usage recorded yet");
        return Ok(());
    }
    let mut n = 0usize;
    let (mut bytes, mut toks) = (0usize, 0usize);
    let mut by_event: HashMap<String, (usize, usize)> = HashMap::new();
    let mut by_client: HashMap<String, (usize, usize)> = HashMap::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    // repos each row id was pushed from — `stats --rows` resolves its
    // open/closed/superseded state through those repos' logs
    let mut id_repos: HashMap<String, Vec<String>> = HashMap::new();
    // (repo, "stop-work"|"stop-bug", ms, session) — checked against each
    // repo's log below; the session joins a block to the round after it
    let mut blocks: Vec<(String, String, i64, String)> = vec![];
    // kept rows feed the chunk-3a ask metrics below (temp-filtered like above)
    let mut kept: Vec<serde_json::Value> = vec![];
    // first usage ts per repo — the "rows added since" denominator
    let mut first_seen: HashMap<String, i64> = HashMap::new();
    // benchmark/test repos live in the OS temp dir and would swamp real usage
    // (01M3CRR6A) — skipped, unless the state dir is scratch too: that run
    // is a test or bench reading its own usage back
    let tmp = [
        std::env::temp_dir(),
        std::env::temp_dir().canonicalize().unwrap_or_default(),
    ];
    let in_tmp = |p: &Path| {
        tmp.iter()
            .any(|t| !t.as_os_str().is_empty() && p.starts_with(t))
    };
    let keep_tmp = in_tmp(&path);
    let mut skipped = 0usize;
    for line in s.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !keep_tmp && v["repo"].as_str().is_some_and(|r| in_tmp(Path::new(r))) {
            skipped += 1;
            continue;
        }
        n += 1;
        kept.push(v.clone());
        let b = v["bytes"].as_u64().unwrap_or(0) as usize;
        let t = v["est_tokens"].as_u64().unwrap_or(0) as usize;
        bytes += b;
        toks += t;
        let ev = v["event"].as_str().unwrap_or("?").to_string();
        let cl = v["client"].as_str().unwrap_or("?").to_string();
        if let (Some(repo), Some(ms)) = (v["repo"].as_str(), v["ts"].as_str().and_then(core::ts_ms))
        {
            first_seen
                .entry(repo.to_string())
                .and_modify(|f| *f = (*f).min(ms))
                .or_insert(ms);
            if ev.starts_with("stop-") {
                blocks.push((
                    repo.to_string(),
                    ev.clone(),
                    ms,
                    v["session"].as_str().unwrap_or("").to_string(),
                ));
            }
        }
        by_event
            .entry(ev)
            .and_modify(|e| {
                e.0 += 1;
                e.1 += t;
            })
            .or_insert((1, t));
        by_client
            .entry(cl)
            .and_modify(|e| {
                e.0 += 1;
                e.1 += t;
            })
            .or_insert((1, t));
        for id in v["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i.as_str())
        {
            *by_id.entry(id.into()).or_insert(0) += 1;
            if let Some(repo) = v["repo"].as_str()
                && !id_repos
                    .get(id)
                    .is_some_and(|rs| rs.iter().any(|r| r == repo))
            {
                id_repos.entry(id.into()).or_default().push(repo.into());
            }
        }
    }
    if n == 0 && !json {
        println!("fael: no usage recorded yet ({skipped} from temp repos skipped)");
        return Ok(());
    }
    // did a row follow each block? work: any add/close · bug: an issue row.
    // ponytail: rows from anyone count — per-session attribution needs `session` on rows
    let mut logs: HashMap<String, core::Log> = HashMap::new();
    let mut outcome: HashMap<String, (usize, usize)> = HashMap::new();
    for (repo, ev, ms, _) in &blocks {
        let log = logs.entry(repo.clone()).or_insert_with(|| repo_log(repo));
        let followed = if ev == "stop-bug" {
            log.rows
                .iter()
                .any(|r| r.kind == "issue" && core::ts_ms(&r.ts).is_some_and(|t| t >= *ms))
        } else {
            core::last_row_ms(log, *ms).is_some()
        };
        let e = outcome.entry(ev.clone()).or_insert((0, 0));
        e.0 += 1;
        e.1 += followed as usize;
    }
    // chunk-3a ask metrics over the same kept rows (PLAN-fael-durable-log §6.2a)
    let ask_rows = askstats::ask_totals(&kept);
    let ask_n = |i: usize| ask_rows[i].1;
    let ask_b = |i: usize| ask_rows[i].2;
    for repo in first_seen.keys() {
        logs.entry(repo.clone()).or_insert_with(|| repo_log(repo));
    }
    let repeat = askstats::repeat_blocks(&blocks, &logs);
    let mut rows_added = 0usize;
    let mut since = i64::MAX;
    for (repo, first) in &first_seen {
        since = since.min(*first);
        if let Some(log) = logs.get(repo) {
            rows_added += askstats::added_since(log, *first);
        }
    }
    let after_block: usize = outcome.values().map(|(_, f)| f).sum();
    let (row_total, foreign_rows) = askstats::non_english_share(&logs);
    let (samples, avg_in, avg_cc, avg_cr, avg_out) = askstats::post_block_cost(&kept);
    let (sk_b, sk_e, sc_b, sc_e) = asks::constants();
    let since_day = if since == i64::MAX {
        String::new()
    } else {
        core::rfc3339(since.max(0) as u64)
            .get(..10)
            .unwrap_or("")
            .to_string()
    };
    if json {
        let top: Vec<_> = {
            let mut v: Vec<_> = by_id.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            v.into_iter()
                .take(10)
                .map(|(id, c)| serde_json::json!({"id": id, "pushes": c}))
                .collect()
        };
        let mut obj = serde_json::json!({
            "events": n, "bytes": bytes, "est_tokens": toks, "skipped_temp": skipped,
            "by_event": by_event.iter().map(|(k, (c, t))| (k.clone(), serde_json::json!({"events": c, "est_tokens": t}))).collect::<serde_json::Map<String,_>>(),
            "by_client": by_client.iter().map(|(k, (c, t))| (k.clone(), serde_json::json!({"events": c, "est_tokens": t}))).collect::<serde_json::Map<String,_>>(),
            "top_rows": top,
            "stop_blocks": outcome.iter().map(|(k, (b, f))| (k.clone(), serde_json::json!({"blocks": b, "followed_by_row": f}))).collect::<serde_json::Map<String,_>>(),
            "asks": ask_rows.iter().map(|(a, c, b)| (a.to_string(), serde_json::json!({"events": c, "bytes": b}))).collect::<serde_json::Map<String,_>>(),
            "repeat_blocks": repeat,
            "constants": {"skill_bytes": sk_b, "skill_est": sk_e, "mcp_schema_bytes": sc_b, "mcp_schema_est": sc_e},
            "rounds": {"after_block": after_block, "rows_added": rows_added, "since": since_day},
            "non_english_rows": {"rows": row_total, "non_english": foreign_rows},
        });
        if samples > 0 {
            obj["real_tokens"] = serde_json::json!({"post_block_rounds": samples,
                "avg_input": avg_in, "avg_cache_create": avg_cc,
                "avg_cache_read": avg_cr, "avg_output": avg_out});
        }
        if rows {
            obj["rows"] = row_report(&by_id, &id_repos)
                .into_iter()
                .map(|(id, pushes, status, noise)| {
                    serde_json::json!({"id": id, "pushes": pushes, "status": status, "noise": noise})
                })
                .collect();
        }
        println!("{obj}");
        return Ok(());
    }
    println!(
        "fael usage ({}): {} injections · {} bytes · ~{} tokens into context",
        path.display(),
        n,
        bytes,
        toks
    );
    if skipped > 0 {
        println!("  skipped ×{skipped} from temp repos (benchmarks, tests)");
    }
    let mut ev: Vec<_> = by_event.iter().collect();
    ev.sort_by_key(|a| std::cmp::Reverse(a.1.0));
    for (k, (c, t)) in ev {
        println!("  {k}: ×{c} (~{t} tokens)");
    }
    let mut cl: Vec<_> = by_client.iter().collect();
    cl.sort_by_key(|a| std::cmp::Reverse(a.1.0));
    for (k, (c, t)) in cl {
        println!("  client {k}: ×{c} (~{t} tokens)");
    }
    let mut ids: Vec<_> = by_id.iter().collect();
    ids.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (id, c) in ids.into_iter().take(10) {
        println!("  row {id}: pushed ×{c}");
    }
    if rows {
        for (id, pushes, status, noise) in row_report(&by_id, &id_repos) {
            println!(
                "  row {id}: pushed ×{pushes} ({status}){}",
                if noise { " noise?" } else { "" }
            );
        }
    }
    let mut oc: Vec<_> = outcome.iter().collect();
    oc.sort();
    for (k, (b, f)) in oc {
        println!("  {k}: {b} block(s) → {f} followed by a row");
    }
    // chunk-3a: what fael asked the agent for, and what a post-block round costs
    println!(
        "  asks: reject ×{} ({} bytes) · stop-block ×{} · warning ×{} ({} bytes)",
        ask_n(0),
        ask_b(0),
        ask_n(1),
        ask_n(2),
        ask_b(2)
    );
    if repeat > 0 {
        println!(
            "  repeat stop-blocks: ×{repeat} (a block followed a block in one session before any row)"
        );
    }
    println!(
        "  constants per session: SKILL.md {sk_b} bytes (~{sk_e} est) + MCP schema {sc_b} bytes (~{sc_e} est)"
    );
    if !blocks.is_empty() {
        println!(
            "  rounds: ~{after_block} row(s) took their own round after a block, of {rows_added} added since {since_day}"
        );
    }
    if row_total > 0 {
        println!("  rows not in English: {foreign_rows} of {row_total}");
    }
    if samples > 0 {
        println!(
            "  post-block rounds: {samples} sample(s), avg ~{} input-side tokens (in {avg_in} + create {avg_cc} + read {avg_cr}; out {avg_out} avg) — the round after a block, not tokens fael used",
            avg_in + avg_cc + avg_cr
        );
    }
    Ok(())
}

/// Per-row push report for `stats --rows`: push counts against the row's
/// current state, most pushed first. `noise?` = pushed ≥ 10 times — the row
/// keeps eating budget without being resolved, so tighten its `--files`
/// scope or close it. Statuses come from the repos the row was pushed from;
/// a repo that is gone (or never had the id) reads `unknown`.
fn row_report(
    by_id: &HashMap<String, usize>,
    id_repos: &HashMap<String, Vec<String>>,
) -> Vec<(String, usize, String, bool)> {
    const TOP: usize = 20;
    const NOISE_PUSHES: usize = 10;
    let mut ids: Vec<_> = by_id.iter().collect();
    ids.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let mut logs: HashMap<String, core::Log> = HashMap::new();
    ids.into_iter()
        .take(TOP)
        .map(|(id, pushes)| {
            let status = id_repos
                .get(id)
                .into_iter()
                .flatten()
                .filter_map(|repo| {
                    let log = logs.entry(repo.clone()).or_insert_with(|| repo_log(repo));
                    let (closed, superseded) = (core::closed(log), core::superseded(log));
                    if closed.contains(id.as_str()) {
                        Some("closed")
                    } else if superseded.contains(id.as_str()) {
                        Some("superseded")
                    } else if log.rows.iter().any(|r| &r.id == id) {
                        Some("open")
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or("unknown")
                .to_string();
            (id.clone(), *pushes, status, *pushes >= NOISE_PUSHES)
        })
        .collect()
}
