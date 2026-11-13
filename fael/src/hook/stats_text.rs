//! The human `fael stats` / `fael stats --day` text — format only, every
//! number comes from `fael-core::stats`. Split out of `usage.rs` (file-size
//! ratchet); `--json` is the contract, this text is unstable by design.

use crate::core;

/// What fael gave back, ahead of what it cost (PLAN-fael-visible-secretary
/// chunk 5) — every number from `--json`, zero parts left out, nothing to
/// say = no line.
pub(super) fn value_line(s: &core::stats::Stats) -> Option<String> {
    let v = &s.value;
    let mut parts: Vec<String> = Vec::new();
    if v.in_context_at_edit > 0 {
        parts.push(format!("in context at edit ×{}", v.in_context_at_edit));
    }
    if v.issues_closed > 0 {
        parts.push(format!("issues closed ×{}", v.issues_closed));
    }
    if v.handoffs_picked_up > 0 {
        let mut detail: Vec<String> = v
            .handoffs_by_client
            .iter()
            .map(|(pair, n)| format!("{pair} {n}"))
            .collect();
        if v.handoffs_same_client > 0 {
            detail.push(format!("same client {}", v.handoffs_same_client));
        }
        if v.handoffs_writer_unknown > 0 {
            detail.push(format!("writer unknown {}", v.handoffs_writer_unknown));
        }
        let base = format!("handoffs picked up ×{}", v.handoffs_picked_up);
        if detail.is_empty() {
            parts.push(base);
        } else {
            parts.push(format!("{base} — {}", detail.join(" · ")));
        }
    }
    if s.retired.at_touch > 0 {
        parts.push(format!("retired at touch ×{}", s.retired.at_touch));
    }
    (!parts.is_empty()).then(|| format!("fael since {}: {}", s.rounds.since, parts.join(" · ")))
}

/// ` · hit 7/638 rows (1%) · acted 40/638 rows (6%)` beside an event's cost,
/// n beside each rate: rows still in context at an edit, and rows also cited,
/// found by id or whose file was read — each over the rows this event handed
/// over first. The first is a lower bound, the second the wider reading (see
/// `core::stats::EventValue`); empty when the event pushed no row.
fn hit_suffix(s: &core::stats::Stats, event: &str) -> String {
    match s.value.by_event.get(event) {
        Some(e) if e.pushed > 0 => {
            let rate = |k: &str, n: usize| {
                format!(" · {k} {n}/{} rows ({}%)", e.pushed, n * 100 / e.pushed)
            };
            rate("hit", e.in_context_at_edit) + &rate("acted", e.acted)
        }
        _ => String::new(),
    }
}

/// One line per usage event: count, cost, and the hit rate beside it.
pub(super) fn print_events(s: &core::stats::Stats) {
    let mut ev: Vec<_> = s.by_event.iter().collect();
    ev.sort_by_key(|a| std::cmp::Reverse(a.1.events));
    for (k, c) in ev {
        let hit = hit_suffix(s, k);
        println!("  {k}: ×{} (~{} tokens){hit}", c.events, c.est_tokens);
    }
    if let Some(l) = latency_line(s) {
        println!("{l}");
    }
}

/// Hook wall time, p50/p95 with n per event — only hooks that put something
/// in context are timed. Nothing timed = no line.
pub(super) fn latency_line(s: &core::stats::Stats) -> Option<String> {
    let parts: Vec<String> = s
        .latency
        .iter()
        .map(|(e, l)| format!("{e} {}/{} (n {})", l.p50, l.p95, l.n))
        .collect();
    (!parts.is_empty()).then(|| format!("  hook time ms p50/p95: {}", parts.join(" · ")))
}

/// PLAN-fael-context-loop §3, read together: repeats over the edits after
/// a close (an upper bound, `ContextLoop`), useful shows per 1k tokens;
/// nothing counted = no line.
pub(super) fn loop_line(s: &core::stats::Stats) -> Option<String> {
    let c = &s.context_loop;
    let none = c.confirmed_repeats + c.edits_after_close + c.useful_shows == 0;
    (!none).then(|| format!(
        "  context loop: confirmed repeats {} of {} edits after close · useful shows {} (~{:.2} per 1k tokens)",
        c.confirmed_repeats,
        c.edits_after_close,
        c.useful_shows,
        c.useful_shows as f64 * 1000.0 / s.est_tokens.max(1) as f64
    ))
}

/// One line on rows that crossed agents; nothing to say = no line.
pub(super) fn cross_line(s: &core::stats::Stats) -> Option<String> {
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
pub(super) fn print_unused(rows: &[core::stats::UnusedRow]) {
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

pub(super) fn print_capture(c: &core::stats::Capture) {
    if let Some(l) = capture_line(c) {
        println!("{l}");
    }
}

/// The capture line as a value, so the short default can tag it.
pub(super) fn capture_line(c: &core::stats::Capture) -> Option<String> {
    if c.manual_adds + c.sessions_with_edits == 0 {
        return None;
    }
    let gone = match c.sessions_with_edits_gone {
        0 => String::new(),
        n => format!(" ({n} in removed worktrees, not judged)"),
    };
    Some(format!(
        "  capture: adds ×{} · {} of {} edited session(s) left no row{gone}",
        c.manual_adds, c.sessions_with_edits_no_row, c.sessions_with_edits
    ))
}

/// The short default's warnings line: the count alone, no codes — the codes
/// live in the warnings section.
pub(super) fn warnings_line(s: &core::stats::Stats) -> Option<String> {
    s.asks
        .get(core::stats::ASK_WARN)
        .filter(|w| w.events > 0)
        .map(|w| format!("  warnings ×{} ({} bytes)", w.events, w.bytes))
}

/// The human `fael stats` text — format only, every number comes from core.
/// ` · shown 2 · cut cap×1 · cited 1 · …`: only the outcomes the row has.
pub(super) fn outcomes_text(o: &core::stats::RowOutcomes) -> String {
    let cut: Vec<String> = o.cut.iter().map(|(r, n)| format!("{r}×{n}")).collect();
    let parts = [
        (o.shown, format!("shown {}", o.shown)),
        (cut.len(), format!("cut {}", cut.join(" "))),
        (o.cited, format!("cited {}", o.cited)),
        (
            o.pulled.agent_initiated + o.pulled.fael_induced,
            format!(
                "pulled {} (+{} fael-induced)",
                o.pulled.agent_initiated, o.pulled.fael_induced
            ),
        ),
        (o.acted, format!("acted {}", o.acted)),
        (
            o.retrieved_after_cut,
            format!("retrieved after cut {}", o.retrieved_after_cut),
        ),
        (o.missed_push, format!("missed push {}", o.missed_push)),
    ];
    parts
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(_, s)| format!(" · {s}"))
        .collect()
}

/// Warning lines by code, most first — none, no line.
pub(super) fn print_warnings(s: &core::stats::Stats) {
    if let Some(w) = s.asks.get(core::stats::ASK_WARN).filter(|w| w.events > 0) {
        println!(
            "  warnings ×{} ({} bytes): {}",
            w.events,
            w.bytes,
            super::friction::counts(&w.codes)
        );
    }
    for l in s.test_leaks.iter().take(3) {
        // a repo-path stream names no session: a test run that forgot FAEL_STATE_DIR.
        // a session-id stream is a live agent: a poll/retry loop, not a test.
        if l.stream.starts_with('/') {
            println!(
                "  warning: `{}` called ×{} within a minute from one stream — looks like a test leak; set FAEL_STATE_DIR",
                l.cmd, l.calls
            );
        } else {
            println!(
                "  warning: `{}` called ×{} within a minute in one agent session — a poll/retry loop, not a test: poll with `fael board --json --watch` or `plan export --since`, with backoff",
                l.cmd, l.calls
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::outcomes_text;
    use crate::core::stats::{Pulled, RowOutcomes};

    #[test]
    fn only_the_outcomes_a_row_has_are_printed() {
        assert_eq!(outcomes_text(&RowOutcomes::default()), "");
        let o = RowOutcomes {
            shown: 2,
            cut: [("cap".to_string(), 1), ("gate".to_string(), 3)].into(),
            pulled: Pulled {
                agent_initiated: 1,
                fael_induced: 2,
            },
            missed_push: 1,
            ..RowOutcomes::default()
        };
        assert_eq!(
            outcomes_text(&o),
            " · shown 2 · cut cap×1 gate×3 · pulled 1 (+2 fael-induced) · missed push 1"
        );
    }
}
