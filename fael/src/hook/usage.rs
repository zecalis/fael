//! Usage accounting (SPEC §8): every injection into context, per machine —
//! never in git. `fael stats` reads it back through `fael-core::stats` (the
//! numbers live in core so the desktop app shares them) and renders here.

use super::asks::{self, UsageMeta};
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

pub fn stats(json: bool, rows: bool) -> Result<(), String> {
    let path = state_dir().join("usage.jsonl");
    let s = std::fs::read_to_string(&path).unwrap_or_default();
    // without `--json` an empty log is one friendly line, not a table of zeros
    if s.trim().is_empty() && !json {
        println!("fael: no usage recorded yet");
        return Ok(());
    }
    // benchmark/test repos live in the OS temp dir (01M3CRR6A) — the boundary
    // rides in as a parameter, so core stays pure
    let tmp = [
        std::env::temp_dir(),
        std::env::temp_dir().canonicalize().unwrap_or_default(),
    ];
    let parsed = core::stats::parse(&s, &path, &tmp);
    if parsed.n == 0 && !json {
        println!(
            "fael: no usage recorded yet ({} from temp repos skipped)",
            parsed.skipped
        );
        return Ok(());
    }
    // finding repos and reading journals stays with the caller (CLI or app);
    // core joins the loaded logs, never the filesystem
    let mut logs: HashMap<String, core::Log> = HashMap::new();
    for repo in parsed.repos() {
        logs.entry(repo.to_string())
            .or_insert_with(|| repo_log(repo));
    }
    let cfg = crate::repo().map(|r| r.cfg).unwrap_or_default();
    let stats = core::stats::aggregate(&parsed, &logs, &cfg, asks::constants().into(), rows);
    if json {
        println!(
            "{}",
            serde_json::to_string(&stats).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    print_text(&stats, &path, &cfg.lang_rows);
    Ok(())
}

/// The human `fael stats` text — format only, every number comes from core.
fn print_text(s: &core::stats::Stats, path: &Path, lang_rows: &[String]) {
    println!(
        "fael usage ({}): {} injections · {} bytes · ~{} tokens into context",
        path.display(),
        s.events,
        s.bytes,
        s.est_tokens
    );
    if s.skipped_temp > 0 {
        println!(
            "  skipped ×{} from temp repos (benchmarks, tests)",
            s.skipped_temp
        );
    }
    let mut ev: Vec<_> = s.by_event.iter().collect();
    ev.sort_by_key(|a| std::cmp::Reverse(a.1.events));
    for (k, c) in ev {
        println!("  {k}: ×{} (~{} tokens)", c.events, c.est_tokens);
    }
    let mut cl: Vec<_> = s.by_client.iter().collect();
    cl.sort_by_key(|a| std::cmp::Reverse(a.1.events));
    for (k, c) in cl {
        println!("  client {k}: ×{} (~{} tokens)", c.events, c.est_tokens);
    }
    for t in s.top_rows.iter().take(10) {
        println!("  row {}: pushed ×{}", t.id, t.pushes);
    }
    if let Some(rows) = &s.rows {
        for r in rows {
            println!(
                "  row {}: pushed ×{} ({}){}",
                r.id,
                r.pushes,
                r.status,
                if r.noise { " noise?" } else { "" }
            );
        }
    }
    for (k, o) in &s.stop_blocks {
        println!(
            "  {k}: {} block(s) → {} followed by a row",
            o.blocks, o.followed_by_row
        );
    }
    let ask = |k: &str| s.asks.get(k).map(|a| (a.events, a.bytes)).unwrap_or((0, 0));
    let (rn, rb) = ask(core::stats::ASK_REJECT);
    let (bn, _) = ask(core::stats::ASK_BLOCK);
    let (wn, wb) = ask(core::stats::ASK_WARN);
    println!("  asks: reject ×{rn} ({rb} bytes) · stop-block ×{bn} · warning ×{wn} ({wb} bytes)");
    if s.repeat_blocks > 0 {
        println!(
            "  repeat stop-blocks: ×{} (a block followed a block in one session before any row)",
            s.repeat_blocks
        );
    }
    println!(
        "  constants per session: SKILL.md {} bytes (~{} est) + MCP schema {} bytes (~{} est)",
        s.constants.skill_bytes,
        s.constants.skill_est,
        s.constants.mcp_schema_bytes,
        s.constants.mcp_schema_est
    );
    if !s.stop_blocks.is_empty() {
        println!(
            "  rounds: ~{} row(s) took their own round after a block, of {} added since {}",
            s.rounds.after_block, s.rounds.rows_added, s.rounds.since
        );
    }
    if s.non_english_rows.rows > 0 && !lang_rows.is_empty() {
        let langs = lang_rows.join("/");
        let label = if langs == "english" {
            "English".to_string()
        } else {
            langs
        };
        println!(
            "  rows not in {label}: {} of {}",
            s.non_english_rows.non_english, s.non_english_rows.rows
        );
    }
    if let Some(r) = &s.real_tokens {
        println!(
            "  post-block rounds: {} sample(s), avg ~{} input-side tokens (in {} + create {} + read {}; out {} avg) — the round after a block, not tokens fael used",
            r.post_block_rounds,
            r.avg_input + r.avg_cache_create + r.avg_cache_read,
            r.avg_input,
            r.avg_cache_create,
            r.avg_cache_read,
            r.avg_output
        );
    }
}
