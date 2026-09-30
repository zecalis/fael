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

pub fn stats(json: bool, rows: bool, day_view: bool) -> Result<(), String> {
    let path = state_dir().join("usage.jsonl");
    let s = std::fs::read_to_string(&path).unwrap_or_default();
    // benchmark/test repos live in the OS temp dir (01M3CRR6A) — the boundary
    // rides in as a parameter, so core stays pure
    let tmp = [
        std::env::temp_dir(),
        std::env::temp_dir().canonicalize().unwrap_or_default(),
    ];
    let parsed = core::stats::parse(&s, &path, &tmp);
    if day_view {
        return day(json, &parsed);
    }
    // without `--json` an empty log is one friendly line, not a table of zeros
    if s.trim().is_empty() && !json {
        println!("fael: no usage recorded yet");
        return Ok(());
    }
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

/// One local day out of usage + logs (`fael stats --day [--json]`): the same
/// `DayView` struct the desktop popover reads over Tauri IPC, printed here
/// so the numbers can be checked against real data before the app exists.
fn day(json: bool, parsed: &core::stats::Parsed) -> Result<(), String> {
    // finding repos and reading journals stays with the caller (CLI or app);
    // core joins the loaded logs, never the filesystem
    let mut logs: HashMap<String, core::Log> = HashMap::new();
    for repo in parsed.repos() {
        logs.entry(repo.to_string())
            .or_insert_with(|| repo_log(repo));
    }
    let me = crate::repo().ok().map(|r| crate::writer(&r));
    let view = core::stats::day(
        parsed,
        &logs,
        me.as_deref(),
        core::now_ms() as i64,
        local_tz_offset_min(),
    );
    if json {
        println!(
            "{}",
            serde_json::to_string(&view).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    print_day(&view);
    Ok(())
}

/// Minutes east of UTC for the day view: `FAEL_TZ_OFFSET` (`+07:00`,
/// `+0700`, `+7`, `Z`) wins so tests and servers pin the day; otherwise the
/// machine's `date +%z` (unix); UTC last.
fn local_tz_offset_min() -> i32 {
    if let Ok(s) = std::env::var("FAEL_TZ_OFFSET")
        && let Some(m) = parse_tz_offset(&s)
    {
        return m;
    }
    #[cfg(unix)]
    if let Ok(o) = std::process::Command::new("date").arg("+%z").output()
        && let Some(m) = parse_tz_offset(String::from_utf8_lossy(&o.stdout).trim())
    {
        return m;
    }
    0
}

fn parse_tz_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let (sign, rest) = match s.strip_prefix('+') {
        Some(r) => (1, r),
        None => (-1, s.strip_prefix('-')?),
    };
    let (h, m) = match rest.split_once(':') {
        Some((h, m)) => (h, m),
        None if rest.len() <= 2 => (rest, "0"),
        None => rest.split_at(rest.len() - 2),
    };
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    (m < 60).then_some(sign * (h * 60 + m))
}

/// The human `fael stats --day` text — format only, every number from core.
fn print_day(v: &core::stats::DayView) {
    let a = &v.all;
    let share = a
        .context
        .share
        .map(|s| format!("{:.2}%", s * 100.0))
        .unwrap_or("—".to_string());
    println!(
        "fael today ({} {}): {} rows delivered · {} fael tokens · share {}",
        v.day, v.tz_offset, a.delivered.rows, a.context.fael_tokens, share
    );
    let mut cl: Vec<_> = a.delivered.by_client.iter().collect();
    cl.sort_by_key(|x| std::cmp::Reverse(x.1));
    let cls: Vec<String> = cl.iter().map(|(k, c)| format!("{k} ×{c}")).collect();
    println!(
        "  delivered: {}{}",
        a.delivered.rows,
        short_list(&cls, " (", ")")
    );
    for l in &a.delivered.last {
        println!("  last: {} → {}", l.title, l.file);
    }
    println!(
        "  context: {} fael / {} session",
        a.context.fael_tokens, a.context.session_tokens
    );
    let mut kinds: Vec<_> = a.memory.added.iter().collect();
    kinds.sort_by_key(|x| std::cmp::Reverse(x.1));
    let adds: Vec<String> = kinds.iter().map(|(k, c)| format!("{k} ×{c}")).collect();
    println!(
        "  memory: {} · closed ×{} · open issues ×{} · superseded ×{}",
        if adds.is_empty() {
            "no rows added".to_string()
        } else {
            format!("+{}", adds.join(" +"))
        },
        a.memory.closed,
        a.memory.open_issues,
        a.memory.superseded
    );
    match &a.for_you {
        Some(f) => {
            let mut from: Vec<_> = f.from.iter().collect();
            from.sort_by_key(|x| std::cmp::Reverse(x.1));
            let fs: Vec<String> = from.iter().map(|(k, c)| format!("{k} ×{c}")).collect();
            println!(
                "  for you: {} rows{} · urgent ×{} · revisit due ×{}",
                f.rows,
                short_list(&fs, " from ", ""),
                f.urgent,
                f.revisit_due
            );
        }
        None => println!("  for you: hidden (no writer set)"),
    }
    println!(
        "  health: {} ignored block(s) · {} stale issue(s)",
        a.health.ignored_blocks, a.health.stale_issues
    );
    for r in &v.repos {
        println!(
            "  repo {}: {} delivered · {} fael tokens · +{} rows",
            r.repo,
            r.panels.delivered.rows,
            r.panels.context.fael_tokens,
            r.panels.memory.added.values().sum::<usize>(),
        );
    }
}

/// `items` joined after `pre` before `post` — nothing at all when empty.
fn short_list(items: &[String], pre: &str, post: &str) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("{pre}{}{post}", items.join(", "))
    }
}
fn print_capture(c: &core::stats::Capture) {
    if c.reply_lines + c.manual_adds + c.sessions_with_edits + c.post_stop_rounds == 0 {
        return;
    }
    println!(
        "  capture: reply ×{} ({} stored, {} rejected) · manual adds ×{} · post-stop rounds ×{} · {} of {} edited session(s) left no row",
        c.reply_lines,
        c.reply_stored,
        c.reply_rejected,
        c.manual_adds,
        c.post_stop_rounds,
        c.sessions_with_edits_no_row,
        c.sessions_with_edits
    );
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
    print_capture(&s.capture);
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
