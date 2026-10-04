//! The human `fael stats` / `fael stats --day` text — format only, every
//! number comes from `fael-core::stats`. Split out of `usage.rs` (file-size
//! ratchet); `--json` is the contract, this text is unstable by design.

use crate::core;
use std::path::Path;

/// What fael gave back, ahead of what it cost (PLAN-fael-visible-secretary
/// chunk 5) — every number from `--json`, zero parts left out, nothing to
/// say = no line.
fn value_line(s: &core::stats::Stats) -> Option<String> {
    let v = &s.value;
    let parts: Vec<String> = [
        ("in context at edit", v.in_context_at_edit),
        ("issues closed", v.issues_closed),
        ("handoffs picked up", v.handoffs_picked_up),
        ("retired at touch", s.retired.at_touch),
        ("filed from replies", s.capture.reply_stored),
    ]
    .iter()
    .filter(|(_, n)| *n > 0)
    .map(|(k, n)| format!("{k} ×{n}"))
    .collect();
    (!parts.is_empty()).then(|| format!("fael since {}: {}", s.rounds.since, parts.join(" · ")))
}

/// ` · hit 7/638 rows (1%)` beside an event's cost: rows still in context at
/// an edit over rows this event handed over first (a lower bound — see
/// `core::stats::EventValue`); empty when the event pushed no row.
fn hit_suffix(s: &core::stats::Stats, event: &str) -> String {
    match s.value.by_event.get(event) {
        Some(e) if e.pushed > 0 => format!(
            " · hit {}/{} rows ({}%)",
            e.in_context_at_edit,
            e.pushed,
            e.in_context_at_edit * 100 / e.pushed
        ),
        _ => String::new(),
    }
}

/// One line per usage event: count, cost, and the hit rate beside it.
fn print_events(s: &core::stats::Stats) {
    let mut ev: Vec<_> = s.by_event.iter().collect();
    ev.sort_by_key(|a| std::cmp::Reverse(a.1.events));
    for (k, c) in ev {
        let hit = hit_suffix(s, k);
        println!("  {k}: ×{} (~{} tokens){hit}", c.events, c.est_tokens);
    }
}

/// One line on rows that crossed agents; nothing to say = no line.
fn cross_line(s: &core::stats::Stats) -> Option<String> {
    let c = &s.value.cross_agent;
    let part = |label: &str, r: &core::stats::Reuse| {
        format!(
            "{label} {} ({} in context at edit)",
            r.pushed, r.in_context_at_edit
        )
    };
    (c.other_session.pushed > 0).then(|| {
        let mut line = format!(
            "  across agents: {} · {} · {}",
            part("from another session", &c.other_session),
            part("another worktree", &c.other_worktree),
            part("written during the session", &c.written_during_session)
        );
        for (pair, r) in &c.by_client {
            line += &format!(" · {}", part(pair, r));
        }
        if c.writer_unknown > 0 {
            line += &format!(" · writer unknown {}", c.writer_unknown);
        }
        line
    })
}

/// The human `fael stats --day` text — format only, every number from core.
pub(super) fn print_day(v: &core::stats::DayView) {
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
    println!("  health: {} stale issue(s)", a.health.stale_issues);
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

/// Open rows pushed over and over, never in context at an edit — the human's
/// to-do (report only: what a push says does not change).
fn print_unused(rows: &[core::stats::UnusedRow]) {
    for r in rows.iter().take(10) {
        println!(
            "  unused {} {}: pushed ×{}, never in context at an edit — fael close {} \"<why>\" · or re-file: fael add {} \"<text>\" --files <files> --supersedes {}",
            r.kind, r.id, r.pushes, r.id, r.kind, r.id
        );
    }
    if rows.len() > 10 {
        println!("  … +{} more (--json: unused_rows)", rows.len() - 10);
    }
}

fn print_capture(c: &core::stats::Capture) {
    if c.reply_lines + c.manual_adds + c.sessions_with_edits == 0 {
        return;
    }
    println!(
        "  capture: reply ×{} ({} stored, {} rejected) · manual adds ×{} · {} of {} edited session(s) left no row",
        c.reply_lines,
        c.reply_stored,
        c.reply_rejected,
        c.manual_adds,
        c.sessions_with_edits_no_row,
        c.sessions_with_edits
    );
}

/// The human `fael stats` text — format only, every number comes from core.
pub(super) fn print_text(s: &core::stats::Stats, path: &Path, lang_rows: &[String]) {
    if let Some(line) = value_line(s) {
        println!("{line}");
    }
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
    print_events(s);
    if let Some(line) = cross_line(s) {
        println!("{line}");
    }
    let mut cl: Vec<_> = s.by_client.iter().collect();
    cl.sort_by_key(|a| std::cmp::Reverse(a.1.events));
    for (k, c) in cl {
        println!("  client {k}: ×{} (~{} tokens)", c.events, c.est_tokens);
    }
    print_unused(&s.unused_rows);
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
    let ask = |k: &str| s.asks.get(k).map(|a| (a.events, a.bytes)).unwrap_or((0, 0));
    let (rn, rb) = ask(core::stats::ASK_REJECT);
    let (wn, wb) = ask(core::stats::ASK_WARN);
    println!("  asks: reject ×{rn} ({rb} bytes) · warning ×{wn} ({wb} bytes)");
    print_capture(&s.capture);
    if s.retired.pushed > 0 {
        println!(
            "  retired at touch: {} of {} pushed row(s) closed or superseded within a day of a push",
            s.retired.at_touch, s.retired.pushed
        );
    }
    println!(
        "  constants per session: SKILL.md {} bytes (~{} est) + MCP schema {} bytes (~{} est)",
        s.constants.skill_bytes,
        s.constants.skill_est,
        s.constants.mcp_schema_bytes,
        s.constants.mcp_schema_est
    );
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
}
