//! `fael report [--out f] [--open] [--since d]` (PLAN-fael-dev-adoption
//! chunk 2): one offline HTML page that answers three questions to take to a
//! lead — what memory reached the agents, what is noise, did fael add
//! friction — and nothing else. Every number is a field of the `Stats` that
//! `fael stats --json` prints for the same window; every row names its id and
//! the command to act on it. Read-only: it writes the page, never `.fael/`.

use crate::{Args, core, hook};
use core::stats::Stats;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;

/// What a pushed row is, looked up in the log of a repo it was pushed from.
pub(crate) struct Info {
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) files: Vec<String>,
}

/// `--since` for `stats`, `tune` and `report`: `None` when absent (this and
/// last month's usage, `usage_files`), `Some(0)` for `all`, an error naming the
/// accepted forms when unreadable.
pub(crate) fn since(a: &Args) -> Result<Option<i64>, String> {
    a.one("since")
        .map(|s| match s.as_str() {
            "all" => Ok(0),
            _ => core::stats::since_arg(&s).ok_or(format!(
                "rejected: --since {s:?} — use YYYY-MM-DD, an RFC 3339 time or all"
            )),
        })
        .transpose()
}

pub(crate) fn report(a: &Args) -> Result<(), String> {
    a.only("report", &["out", "open", "since"])?;
    let u = hook::load(since(a)?);
    let cfg = crate::repo().map(|r| r.cfg).unwrap_or_default();
    let s = hook::aggregate(&u, &cfg, true);
    let mut info = HashMap::new();
    for r in s.rows.iter().flatten() {
        let found = u
            .parsed
            .id_repos
            .get(&r.id)
            .into_iter()
            .flatten()
            .find_map(|repo| {
                u.logs
                    .get(repo)?
                    .rows
                    .iter()
                    .rev()
                    .find(|row| row.id == r.id)
            });
        if let Some(row) = found {
            info.insert(
                r.id.clone(),
                Info {
                    kind: row.kind.clone(),
                    title: row.display_title(),
                    files: row.files.clone(),
                },
            );
        }
    }
    let window = match a.one("since") {
        Some(d) if d == "all" => "all recorded usage".into(),
        Some(d) => format!("since {d}"),
        None if s.rounds.since.is_empty() => "this and last month".into(),
        None => format!("since {} (this and last month)", s.rounds.since),
    };
    let flag = a
        .one("since")
        .map(|d| format!(" --since {d}"))
        .unwrap_or_default();
    let out = a
        .one("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| core::stats::state_dir().join("report.html"));
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&out, render(&s, &info, &window, &flag))
        .map_err(|e| format!("{}: {e}", out.display()))?;
    println!("fael: report written to {}", out.display());
    if a.has("open") {
        open(&out);
    }
    Ok(())
}

/// The OS opener; a failure is one hint line, never an error — the file is
/// already written.
fn open(path: &std::path::Path) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if std::process::Command::new(cmd).arg(path).status().is_err() {
        println!("fael: could not run {cmd} — open the file yourself");
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = ":root{--bg:#fff;--fg:#1d1d1f;--mute:#6e6e73;--line:#e5e5ea;--code:#f2f2f7}
@media (prefers-color-scheme:dark){:root{--bg:#161618;--fg:#f2f2f7;--mute:#a1a1a6;--line:#2c2c2e;--code:#232326}}
body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}
main{max-width:960px;margin:0 auto;padding:24px 16px}
h1{margin:0 0 4px}h2{margin:32px 0 8px;font-size:18px}
.sub,.none,footer{color:var(--mute)}
code{background:var(--code);padding:1px 4px;border-radius:4px;font-size:13px}
.wrap{overflow-x:auto}
table{border-collapse:collapse;width:100%}
th,td{text-align:left;padding:6px 8px;border-bottom:1px solid var(--line);vertical-align:top}
th{color:var(--mute);font-weight:500}td.n{text-align:right}";

/// Pure: the whole page from `Stats` plus what each pushed row is. `window`
/// says which usage the numbers cover, `flag` the `--since` that reproduces
/// them in `fael stats --json`.
pub(crate) fn render(s: &Stats, info: &HashMap<String, Info>, window: &str, flag: &str) -> String {
    let mut h = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>fael report</title><style>{STYLE}</style></head><body><main>\n\
         <h1>fael report</h1>\n<p class=\"sub\">{} · {} usage events on this machine · \
         every number comes from <code>fael stats --json{}</code></p>\n",
        esc(window),
        s.events,
        esc(flag)
    );
    delivered(&mut h, s, info);
    noise(&mut h, s, info);
    friction(&mut h, s);
    h.push_str(
        "<footer><p>Offline, no scripts. Estimates carry ~ — they come from text length, \
         not a tokenizer.</p></footer>\n</main></body></html>\n",
    );
    h
}

fn delivered(h: &mut String, s: &Stats, info: &HashMap<String, Info>) {
    let ev = |k: &str| {
        s.by_event
            .get(k)
            .map(|c| (c.events, c.est_tokens))
            .unwrap_or((0, 0))
    };
    let parts = ["read", "edit", "session-start", "search", "prompt"].map(ev);
    let (n, toks) = parts.iter().fold((0, 0), |a, p| (a.0 + p.0, a.1 + p.1));
    let _ = write!(
        h,
        "<section><h2>1. What memory reached your agents?</h2>\n<p>fael put memory into \
         context <b>{n}</b> times (read ×{} · edit ×{} · session start ×{} · search ×{} · \
         prompt hint ×{}), ~{toks} tokens.",
        parts[0].0, parts[1].0, parts[2].0, parts[3].0, parts[4].0
    );
    if s.retired.pushed > 0 {
        let _ = write!(
            h,
            " Of {} rows pushed on a read or edit, {} were closed or superseded within a day of \
             the push.",
            s.retired.pushed, s.retired.at_touch
        );
    }
    if s.value.in_context_at_edit > 0 {
        let _ = write!(
            h,
            " Decisions or issues a push handed over and still in context when the agent edited \
             their file: {} (each counted once per session).",
            s.value.in_context_at_edit
        );
    }
    h.push_str("</p>\n");
    let rows: Vec<_> = s.rows.iter().flatten().take(10).collect();
    table(
        h,
        &rows,
        info,
        |r| format!("fael find {}", r.id),
        "No row was pushed in this window.",
    );
    h.push_str("</section>\n");
}

fn noise(h: &mut String, s: &Stats, info: &HashMap<String, Info>) {
    h.push_str(
        "<section><h2>2. What is noise?</h2>\n<p>Open rows pushed 10 times or more. \
         Each one costs context on every push — check it still points the right way: \
         close it when the code already says it, or file it again with narrower \
         <code>--files</code>.</p>\n",
    );
    let rows: Vec<_> = s
        .rows
        .iter()
        .flatten()
        .filter(|r| r.noise && r.status == "open")
        .collect();
    table(
        h,
        &rows,
        info,
        |r| format!("fael close {} \"<why>\"", r.id),
        "No open row was pushed 10 times or more.",
    );
    h.push_str("</section>\n");
}

fn table(
    h: &mut String,
    rows: &[&core::stats::RowStatus],
    info: &HashMap<String, Info>,
    next: impl Fn(&core::stats::RowStatus) -> String,
    none: &str,
) {
    if rows.is_empty() {
        let _ = writeln!(h, "<p class=\"none\">{none}</p>");
        return;
    }
    h.push_str(
        "<div class=\"wrap\"><table><thead><tr><th>row</th><th>files</th><th>status</th>\
         <th>pushes</th><th>next</th></tr></thead><tbody>\n",
    );
    for r in rows {
        let (what, files) = match info.get(&r.id) {
            Some(i) => (
                format!("{}: {}", i.kind, i.title),
                i.files
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
                    + if i.files.len() > 2 { ", …" } else { "" },
            ),
            None => ("(not in a log on this machine)".into(), String::new()),
        };
        let _ = writeln!(
            h,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"n\">{}</td><td><code>{}</code></td></tr>",
            esc(&what),
            esc(&files),
            esc(&r.status),
            r.pushes,
            esc(&next(r))
        );
    }
    h.push_str("</tbody></table></div>\n");
}

fn friction(h: &mut String, s: &Stats) {
    let c = &s.capture;
    let ask = |k: &str| s.asks.get(k).map(|a| a.events).unwrap_or(0);
    let _ = write!(
        h,
        "<section><h2>3. Did fael add friction?</h2>\n<ul>\n\
         <li>Memory written from replies (<code>fael decision: …</code> lines): {} stored · {} rejected</li>\n\
         <li>Rows written outside replies (<code>add</code> over CLI or MCP, synced teammate rows included): {}</li>\n\
         <li>Sessions that edited files: {}, of which {} left no row</li>\n\
         <li>Asks: reject ×{} · warning ×{}</li>\n</ul>\n\
         <p>fael never blocks a turn, so it adds no round after the agent is done.</p>\n\
         </section>\n",
        c.reply_stored,
        c.reply_rejected,
        c.manual_adds,
        c.sessions_with_edits,
        c.sessions_with_edits_no_row,
        ask(core::stats::ASK_REJECT),
        ask(core::stats::ASK_WARN),
    );
}

#[cfg(test)]
mod tests {
    use super::{Info, render};
    use crate::core;
    use std::collections::HashMap;

    /// Golden page: `FAEL_BLESS=1 cargo test -p fael report` rewrites it.
    #[test]
    fn report_matches_the_golden_page() {
        let text = concat!(
            "{\"ts\":\"2026-09-30T09:00:00.000Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":40,\"est_tokens\":10,\"ids\":[\"A\",\"B\"]}\n",
            "{\"ts\":\"2026-09-30T09:01:00.000Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"edit\",\"bytes\":20,\"est_tokens\":5,\"ids\":[\"A\"]}\n",
            "{\"ts\":\"2026-09-30T09:02:00.000Z\",\"repo\":\"/work/real\",\"client\":\"cli\",\"event\":\"add\",\"ask\":\"warning\",\"bytes\":8,\"est_tokens\":2,\"ids\":[]}\n",
        );
        let p = core::stats::parse(text, std::path::Path::new("/work/state/usage.jsonl"), &[]);
        let mut s = core::stats::aggregate(
            &p,
            &HashMap::new(),
            &core::Config::default(),
            (0, 0, 0, 0).into(),
            true,
        );
        // statuses come from logs this fixture has none of — set them by hand
        for r in s.rows.iter_mut().flatten() {
            r.status = "open".into();
            r.noise = r.id == "A";
        }
        let info = HashMap::from([(
            "A".to_string(),
            Info {
                kind: "decision".into(),
                title: "Cache keys <include> the tenant".into(),
                files: vec!["src/a.rs".into(), "src/b.rs".into(), "src/c.rs".into()],
            },
        )]);
        let page = render(&s, &info, "since 2026-09-30", " --since 2026-09-30");
        let golden = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/report.html");
        if std::env::var_os("FAEL_BLESS").is_some() {
            std::fs::create_dir_all(std::path::Path::new(golden).parent().unwrap()).unwrap();
            std::fs::write(golden, &page).unwrap();
        }
        assert_eq!(page, std::fs::read_to_string(golden).unwrap_or_default());
    }
}
