//! `fael stats --session <id>` and `fael stats sessions` — text only, every
//! number from `fael-core::stats::sessions` (issue 01M75KQ5). `--json` is the
//! contract; this text is unstable by design.

use crate::core::stats::{Group, SessionView, Split, Splits};

/// The fixed line under each split table.
const DOES_NOT_SHOW: &str =
    "  does not show: an effect of fael — the groups differ in task, client and repo";

/// One session, line per story.
pub(super) fn session_lines(v: &SessionView) -> Vec<String> {
    let total: usize = v.pushed_tokens.values().sum();
    let by_event = v
        .pushed_tokens
        .iter()
        .map(|(e, t)| format!("{e} ~{t}"))
        .collect::<Vec<_>>()
        .join(", ");
    let pr = match (&v.branch, &v.pr) {
        (None, _) => "no branch (it never edited)".to_string(),
        (_, None) => "no PR listed".to_string(),
        (_, Some(p)) => format!(
            "PR #{} {} (its branch's PR; it may hold other sessions' edits too)",
            p.number,
            p.merged
                .as_ref()
                .map_or("not merged".into(), |d| format!("merged {d}"))
        ),
    };
    // pushed est tokens against the transcript's input side (in + cache); the
    // two are different measures (est is once, cache reads recount per turn)
    let tokens = match &v.tokens {
        None => "  session tokens: n/a (no transcript total from this client)".to_string(),
        Some(t) => {
            let cache = t.cache_creation + t.cache_read;
            format!(
                "  session tokens: in {} · cache {cache} · out {}",
                t.input, t.output
            )
        }
    };
    let pct = match &v.tokens {
        Some(t) if t.input + t.cache_creation + t.cache_read > 0 => format!(
            " ({:.2}% of input)",
            total as f64 * 100.0 / (t.input + t.cache_creation + t.cache_read) as f64
        ),
        _ => String::new(),
    };
    vec![
        format!("session {} ({})", v.session, v.repo),
        format!(
            "  client {} · {} → {}{}",
            v.client,
            v.first,
            v.last,
            v.branch
                .as_ref()
                .map(|b| format!(" · branch {b}"))
                .unwrap_or_default()
        ),
        tokens,
        format!("  {pr}"),
        format!(
            "  fael pushed: {total} est tokens{pct}{}",
            if by_event.is_empty() {
                String::new()
            } else {
                format!(": {by_event}")
            }
        ),
        format!(
            "  rows shown {} · of {} scored: in context at an edit {} · acted {}",
            v.shown, v.scored, v.in_context, v.acted
        ),
        format!(
            "  edits {} · calls {} · rejects {} · rows filed {}",
            v.edits,
            v.calls,
            v.rejects,
            v.rows_filed
                .map_or("unknown (log not loaded)".into(), |n| n.to_string())
        ),
    ]
}

fn group_line(label: &str, g: &Group) -> String {
    let rate = if g.calls == 0 {
        "n/a".to_string()
    } else {
        format!("{:.1}", g.rejects as f64 * 100.0 / g.calls as f64)
    };
    format!(
        "  {label}: sessions {} · edits {} · rejects per 100 calls {rate} ({} calls) · rows filed {} · no row {} · with a merged PR {}",
        g.sessions, g.edits, g.calls, g.rows_filed, g.no_row, g.merged_pr
    )
}

fn table(title: &str, (with, without): (&str, &str), s: &Split) -> [String; 4] {
    [
        title.to_string(),
        group_line(with, &s.with),
        group_line(without, &s.without),
        DOES_NOT_SHOW.to_string(),
    ]
}

/// The two split tables of `fael stats sessions`.
pub(super) fn split_lines(s: &Splits) -> Vec<String> {
    let a = table(
        "sessions with an edit, rows pushed ≥ 1 against none:",
        ("rows pushed", "none pushed"),
        &s.pushed,
    );
    let b = table(
        "sessions with an edit, inside a chunk run against outside:",
        ("inside a run", "outside"),
        &s.in_run,
    );
    a.into_iter().chain(b).collect()
}
