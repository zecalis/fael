//! The `fael stats` page: its sections, the short default, and the friction
//! total line. Split out of `stats_text.rs` (file-size ratchet) — the small
//! per-story line builders stay there; this file only selects and prints.
//! `--json` is the contract, this text is unstable by design.
//! New surface for issue 01M74SGCWBWT0M2PQ9N0AKHRVR (`--all` + sections).

use super::stats_text;
use crate::core;
use std::path::Path;

/// Sections of the full `fael stats` page: `fael stats <section>` prints one,
/// `fael stats --all` prints all. The sections partition the full page — every
/// line it prints belongs to exactly one — so their union is the old default,
/// line for line. The short default prints one short line per story instead,
/// each ending in its section name.
pub(super) const SECTIONS: &[&str] = &[
    "rows", "value", "outcome", "across", "models", "capture", "said", "friction", "warnings",
];

fn usage_line(s: &core::stats::Stats, path: &Path) -> String {
    format!(
        "fael usage ({}): {} injections · {} bytes · ~{} tokens into context",
        path.display(),
        s.events,
        s.bytes,
        s.est_tokens
    )
}

fn skipped_line(s: &core::stats::Stats) -> Option<String> {
    (s.skipped_temp > 0).then(|| {
        format!(
            "  skipped ×{} from temp repos (benchmarks, tests)",
            s.skipped_temp
        )
    })
}

fn same_file_line(s: &core::stats::Stats) -> Option<String> {
    let f = &s.value.cross_agent.same_file;
    (f.files > 0).then(|| {
        format!(
            "  same file within {} min: {} files · {} session pairs (of {} sessions with edits seen)",
            core::stats::OVERLAP_WINDOW_MS / 60_000,
            f.files,
            f.session_pairs,
            f.sessions_seen
        )
    })
}

fn retired_line(s: &core::stats::Stats) -> Option<String> {
    (s.retired.pushed > 0).then(|| {
        format!(
            "  retired at touch: {} of {} pushed row(s) closed or superseded within a day of a push · bumped {}",
            s.retired.at_touch, s.retired.pushed, s.retired.bumped
        )
    })
}

fn constants_line(s: &core::stats::Stats) -> String {
    format!(
        "  constants per session: SKILL.md {} bytes (~{} est) + MCP schema {} bytes (~{} est)",
        s.constants.skill_bytes,
        s.constants.skill_est,
        s.constants.mcp_schema_bytes,
        s.constants.mcp_schema_est
    )
}

fn non_english_line(s: &core::stats::Stats, lang_rows: &[String]) -> Option<String> {
    if s.non_english_rows.rows > 0 && !lang_rows.is_empty() {
        let langs = lang_rows.join("/");
        let label = if langs == "english" {
            "English".to_string()
        } else {
            langs
        };
        return Some(format!(
            "  rows not in {label}: {} of {}",
            s.non_english_rows.non_english, s.non_english_rows.rows
        ));
    }
    None
}

fn row_line(r: &core::stats::RowStatus) -> String {
    format!(
        "  row {}: pushed ×{} ({}){}{}",
        r.id,
        r.pushes,
        r.status,
        if r.noise { " noise?" } else { "" },
        stats_text::outcomes_text(&r.outcomes)
    )
}

/// `per 100 calls`, one decimal.
fn per100(n: usize, calls: usize) -> String {
    format!("{:.1}", n as f64 * 100.0 / calls as f64)
}

/// A tally's rate alone, without its reject reasons — shared by the full
/// total line and the short default's.
pub(super) fn tally_rate(t: &core::stats::Tally) -> String {
    let (c, bad) = (t.calls, t.rejects + t.help + t.find_repeat);
    format!(
        "{} — reject {} · help {} · find repeat {} · first-call success {}%",
        per100(bad, c),
        per100(t.rejects, c),
        per100(t.help, c),
        per100(t.find_repeat, c),
        t.first_call_ok * 100 / c
    )
}

/// The short default's friction line: the total rate alone — no per-reason
/// list, no gate rules (both live in the friction section). `None` when
/// there were no calls at all (and no lone gate line to print).
pub(super) fn total_line(f: &core::stats::Friction) -> Option<String> {
    let gate = match f.shape_gate {
        0 => String::new(),
        n => format!(" · shape gate ×{n} (not friction)"),
    };
    if f.total.calls == 0 {
        return match f.shape_gate {
            0 => None,
            _ => Some(format!("  friction per 100 calls (0 calls): n/a{gate}")),
        };
    }
    Some(format!(
        "  friction per 100 calls ({} calls): {}{gate}",
        f.total.calls,
        tally_rate(&f.total),
    ))
}

pub(super) fn print_all(s: &core::stats::Stats, path: &Path, lang_rows: &[String]) {
    super::lifecycle::print(&s.lifecycle);
    if let Some(line) = stats_text::value_line(s) {
        println!("{line}");
    }
    if let Some(line) = super::stats_outcome::outcome_line(s) {
        println!("{line}");
    }
    println!("{}", usage_line(s, path));
    if let Some(line) = skipped_line(s) {
        println!("{line}");
    }
    stats_text::print_events(s);
    if let Some(line) = stats_text::loop_line(s) {
        println!("{line}");
    }
    for line in super::experience_text::experience_lines(s) {
        println!("{line}");
    }
    if let Some(line) = stats_text::cross_line(s) {
        println!("{line}");
    }
    if let Some(line) = super::stats_handoff::handoff_line(&s.handoff_runs) {
        println!("{line}");
    }
    if let Some(line) = same_file_line(s) {
        println!("{line}");
    }
    super::stats_models::print_attribution(s);
    stats_text::print_unused(&s.unused_rows);
    if let Some(rows) = &s.rows {
        for r in rows {
            println!("{}", row_line(r));
        }
    }
    stats_text::print_warnings(s);
    stats_text::print_capture(&s.capture);
    if let Some(line) = retired_line(s) {
        println!("{line}");
    }
    for line in [
        super::said_text::said_line(&s.said),
        super::said_text::said_events_line(&s.said),
        super::said_text::notes_line(&s.said),
    ]
    .into_iter()
    .flatten()
    {
        println!("{line}");
    }
    super::friction::lines(&s.friction)
        .iter()
        .for_each(|l| println!("{l}"));
    super::flag_usage::lines(&s.usage)
        .iter()
        .for_each(|l| println!("{l}"));
    println!("{}", constants_line(s));
    if let Some(line) = non_english_line(s, lang_rows) {
        println!("{line}");
    }
}

/// One section of the full page: the same lines `print_all` prints for it,
/// in the same order.
pub(super) fn print_section(
    s: &core::stats::Stats,
    path: &Path,
    lang_rows: &[String],
    section: &str,
) {
    match section {
        "rows" => {
            super::lifecycle::print(&s.lifecycle);
            println!("{}", usage_line(s, path));
            if let Some(line) = skipped_line(s) {
                println!("{line}");
            }
            stats_text::print_events(s);
            stats_text::print_unused(&s.unused_rows);
            if let Some(rows) = &s.rows {
                for r in rows {
                    println!("{}", row_line(r));
                }
            }
            if let Some(line) = retired_line(s) {
                println!("{line}");
            }
            println!("{}", constants_line(s));
            if let Some(line) = non_english_line(s, lang_rows) {
                println!("{line}");
            }
        }
        "value" => {
            if let Some(line) = stats_text::value_line(s) {
                println!("{line}");
            }
            if let Some(line) = stats_text::loop_line(s) {
                println!("{line}");
            }
            for line in super::experience_text::experience_lines(s) {
                println!("{line}");
            }
        }
        "outcome" => {
            if let Some(line) = super::stats_outcome::outcome_line(s) {
                println!("{line}");
            }
            if let Some(line) = super::stats_handoff::handoff_line(&s.handoff_runs) {
                println!("{line}");
            }
            if let Some(line) = same_file_line(s) {
                println!("{line}");
            }
        }
        "across" => {
            if let Some(line) = stats_text::cross_line(s) {
                println!("{line}");
            }
        }
        "models" => super::stats_models::print_attribution(s),
        "capture" => stats_text::print_capture(&s.capture),
        "said" => {
            for line in [
                super::said_text::said_line(&s.said),
                super::said_text::said_events_line(&s.said),
                super::said_text::notes_line(&s.said),
            ]
            .into_iter()
            .flatten()
            {
                println!("{line}");
            }
        }
        "friction" => {
            super::friction::lines(&s.friction)
                .iter()
                .for_each(|l| println!("{l}"));
            super::flag_usage::lines(&s.usage)
                .iter()
                .for_each(|l| println!("{l}"));
        }
        "warnings" => stats_text::print_warnings(s),
        _ => {}
    }
}

/// The short default: one short line per story, each ending in its section
/// name — the full story stays one `fael stats <section>` away.
pub(super) fn print_default(s: &core::stats::Stats, path: &Path) {
    for l in super::lifecycle::lines(&s.lifecycle) {
        println!("{l} (rows)");
    }
    println!("{} (rows)", usage_line(s, path));
    if let Some(l) = skipped_line(s) {
        println!("{l} (rows)");
    }
    if let Some(l) = stats_text::value_line(s) {
        println!("{l} (value)");
    }
    if let Some(l) = super::stats_outcome::outcome_line(s) {
        println!("{l} (outcome)");
    }
    if let Some(l) = super::said_text::said_line_top(&s.said, 5) {
        println!("{l} (said)");
    }
    if let Some(l) = total_line(&s.friction) {
        println!("{l} (friction)");
    }
    if let Some(l) = stats_text::capture_line(&s.capture) {
        println!("{l} (capture)");
    }
    if let Some(l) = stats_text::warnings_line(s) {
        println!("{l} (warnings)");
    }
    if let Some(rows) = &s.rows {
        for r in rows {
            println!("{}", row_line(r));
        }
    }
}
