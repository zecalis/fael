//! `fael report` §§4–6: session splits, recent sessions, provenance.
//! Split out of `super` for the 400-line ratchet — pure HTML from `Stats`.

use fael_core::stats::Stats;
use std::fmt::Write as _;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn splits(h: &mut String, s: &Stats) {
    h.push_str("<section><h2>4. Sessions with an edit, split two ways</h2>\n");
    for (t, wl, ol, sp) in [
        (
            "rows pushed ≥1 against none",
            "rows pushed",
            "none pushed",
            &s.session_splits.pushed,
        ),
        (
            "inside a chunk run against outside",
            "inside a run",
            "outside",
            &s.session_splits.in_run,
        ),
    ] {
        let _ = write!(
            h,
            "<p>{t}:</p>\n<div class=\"wrap\"><table><thead><tr><th>group</th><th>sessions</th><th>edits</th><th>calls</th><th>rejects/100</th><th>rows filed</th><th>no row</th><th>merged PR</th></tr></thead><tbody>\n"
        );
        for (l, g) in [(wl, &sp.with), (ol, &sp.without)] {
            let r = if g.calls == 0 {
                "n/a".into()
            } else {
                format!("{:.1}", g.rejects as f64 * 100.0 / g.calls as f64)
            };
            let _ = writeln!(
                h,
                "<tr><td>{l}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{r}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>",
                g.sessions, g.edits, g.calls, g.rows_filed, g.no_row, g.merged_pr
            );
        }
        h.push_str("</tbody></table></div>\n<p class=\"none\">does not show: an effect of fael — the groups differ in task, client and repo</p>\n");
    }
    h.push_str("</section>\n");
}

pub(crate) fn recent(h: &mut String, s: &Stats) {
    h.push_str("<section><h2>5. Ten most recent sessions</h2>\n");
    if s.sessions.is_empty() {
        h.push_str("<p class=\"none\">No session in this window.</p>\n</section>\n");
        return;
    }
    h.push_str("<div class=\"wrap\"><table><thead><tr><th>session</th><th>pushed</th><th>rows</th><th>PR</th><th>transcript</th></tr></thead><tbody>\n");
    for v in s.sessions.iter().take(10) {
        let total: usize = v.pushed_tokens.values().sum();
        let mut ev: Vec<_> = v
            .pushed_tokens
            .iter()
            .map(|(e, t)| format!("{e} ~{t}"))
            .collect();
        ev.sort();
        let pushed = if ev.is_empty() {
            format!("{total} est")
        } else {
            format!("{total} est: {}", ev.join(", "))
        };
        let rows = format!(
            "shown {} · scored {} · in context {} · acted {} · edits {} · calls {} · rejects {}",
            v.shown, v.scored, v.in_context, v.acted, v.edits, v.calls, v.rejects
        );
        let pr = match (&v.branch, &v.pr) {
            (None, _) => "no branch (it never edited)".into(),
            _ if !v.pr_checked => "PR unknown (no PR list for this repo)".into(),
            (_, None) => format!(
                "no PR listed{}",
                v.branch
                    .as_ref()
                    .map(|b| format!(" (branch {b})"))
                    .unwrap_or_default()
            ),
            (_, Some(p)) => format!(
                "PR #{} {}",
                p.number,
                p.merged
                    .as_ref()
                    .map(|d| format!("merged {d}"))
                    .unwrap_or_else(|| "not merged".into())
            ),
        };
        let tx = match &v.tokens {
            None => "n/a".into(),
            Some(t) => format!(
                "in {} · cache {} · out {}",
                t.input,
                t.cache_creation + t.cache_read,
                t.output
            ),
        };
        let _ = writeln!(
            h,
            "<tr><td>{} ({})<br>{} → {}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&v.session),
            esc(&v.client),
            esc(&v.first),
            esc(&v.last),
            esc(&pushed),
            esc(&rows),
            esc(&pr),
            esc(&tx)
        );
    }
    h.push_str("</tbody></table></div>\n</section>\n");
}

pub(crate) fn provenance(h: &mut String, s: &Stats) {
    h.push_str(
        "<section><h2>6. Provenance</h2>\n<p>How to recompute every git/gh-backed number.</p>\n",
    );
    if s.provenance.is_empty() {
        h.push_str(
            "<p class=\"none\">No provenance (core alone built the stats).</p>\n</section>\n",
        );
        return;
    }
    h.push_str("<div class=\"wrap\"><table><thead><tr><th>field</th><th>shell line</th></tr></thead><tbody>\n");
    for (k, v) in &s.provenance {
        let _ = writeln!(
            h,
            "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>",
            esc(k),
            esc(v)
        );
    }
    h.push_str("</tbody></table></div>\n</section>\n");
}
