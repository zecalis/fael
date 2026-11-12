//! Usage accounting (SPEC §8): every injection into context, per machine —
//! never in git. `fael stats` reads it back through `fael-core::stats` (the
//! numbers live in core so the desktop app shares them) and renders here.

use super::asks::{self, UsageMeta};
use super::state::{head_branch, now_rfc3339};
use super::stats_text::print_day;
use super::usage_commits::{commits, commits_by_clone};
use super::usage_load::{Usage, load_text, load_where, repo_log};
use crate::core;
use std::path::Path;

/// Every injection into context, per machine — never in git. Fails open:
/// a usage write never fails the command it rode along with. Asks (reject /
/// warning), the session, and real transcript tokens ride in
/// `meta` — absent keys stay absent, so old readers keep working.
pub(crate) fn record_usage(
    client: &str,
    event: &str,
    repo: &Path,
    text: &str,
    ids: &[String],
    meta: &UsageMeta,
) {
    asks::append_row(usage_row(client, event, repo, text, ids, meta));
}

/// A pull that showed rows (`find`, `mcp-find`, `kickoff`; PLAN-fael-say-gate
/// chunk 3): the outcome a Pointer, Count or Bodies line (lines no hook says now) earned on. The shown
/// ids go under `found`, never `ids` (those count as pushes), and stats keeps
/// the line out of the injection totals. The query's key, files and id only —
/// free text is never stored (`find-misses.jsonl` holds the misses). `text`
/// is what the agent saw: its bytes and tokens are the pull's cost.
pub(crate) fn record_found(
    client: &str,
    event: &str,
    root: &Path,
    text: &str,
    found: &[String],
    (key, files, id): (Option<&str>, &[String], Option<&str>),
) {
    if found.is_empty() {
        return;
    }
    let session = crate::session::hook_session(root);
    let meta = UsageMeta {
        session: (!session.is_empty()).then_some(session.as_str()),
        ..UsageMeta::default()
    };
    let mut row = usage_row(client, event, root, text, &[], &meta);
    row["found"] = found.into();
    let mut q = serde_json::json!({});
    if let Some(k) = key {
        q["key"] = k.into();
    }
    if !files.is_empty() {
        q["files"] = files.into();
    }
    if let Some(i) = id {
        q["id"] = i.into();
    }
    row["q"] = q;
    asks::append_row(row);
}

/// One usage row, every key `record_usage` writes — shared with the 0-byte
/// `in-context-at-edit` row so the two shapes cannot drift.
pub(crate) fn usage_row(
    client: &str,
    event: &str,
    repo: &Path,
    text: &str,
    ids: &[String],
    meta: &UsageMeta,
) -> serde_json::Value {
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
    if let Some(agent) = meta.agent {
        row["agent"] = agent.into();
    }
    // worktrees share one journal: the branch is what ties an edit session
    // to the rows it filed (stats capture). A file read, no git spawn.
    if event == "edit"
        && let Some(b) = head_branch(repo)
    {
        row["branch"] = b.into();
    }
    // an edit names its files: two sessions at one file is only visible here
    if !meta.files.is_empty() {
        row["files"] = meta.files.into();
    }
    if let Some(real) = meta.real
        && let Ok(t) = serde_json::to_value(real)
    {
        row["real_tokens"] = t;
    }
    if !meta.said.is_empty()
        && let Ok(s) = serde_json::to_value(meta.said)
    {
        row["said"] = s;
    }
    if let Some(d) = meta.decision.and_then(|d| d.as_object()) {
        for (k, v) in d {
            row[k] = v.clone();
        }
    }
    row
}

/// The `Stats` `fael stats --json` prints — `fael report` renders the same one.
pub(crate) fn aggregate(u: &Usage, cfg: &core::Config, rows: bool) -> core::stats::Stats {
    // The repo `fael stats` runs in is read even when no usage line names it,
    // so a fresh clone's outcome reads log + git + gh alone (outcome-dayone).
    let cwd = crate::repo()
        .ok()
        .map(|r| r.root.to_string_lossy().into_owned())
        .filter(|k| !u.logs.contains_key(k));
    let mut owned;
    let logs = match &cwd {
        Some(k) => {
            owned = u.logs.clone();
            owned.insert(k.clone(), repo_log(k));
            &owned
        }
        None => &u.logs,
    };
    let first_seen = u.parsed.first_seen.values().copied().min();
    let window = core::stats::outcome_window(u.since, logs, first_seen);
    let mut commit_map = commits_by_clone(u);
    if let Some(k) = &cwd {
        let found = commits(Path::new(k), window.unwrap_or(0));
        commit_map.insert(k.clone(), found.into_iter().map(|(_, c)| c).collect());
    }
    let constants = asks::constants().into();
    let runs = super::flow::runs();
    let merges = super::flow::merges(window);
    let tz = local_tz_offset_min();
    let (prs, prs_merged_total) = crate::maintain::outcome_inputs(u.since, logs, tz, first_seen);
    let mut s = core::stats::aggregate(
        &u.parsed,
        logs,
        &commit_map,
        &runs,
        &merges,
        cfg,
        constants,
        rows,
        tz,
        u.since,
        &prs,
        prs_merged_total,
    );
    // sessions of this clone join its PR list (other repos stay unknown)
    if let Ok(r) = crate::repo() {
        s.session_splits = core::stats::link_prs(&mut s.sessions, &prs, &r.root.to_string_lossy());
    }
    // this clone's run attribution (client/model/cost with unknown-% beside
    // every number); no crew.db reads empty, never an error
    s.models = core::stats::models(&super::flow::attribution());
    // provenance: field path → the exact shell line with this run's range
    // filled in (plan proof chunk `provenance`); the range is the same
    // string `prs_merged_total` was counted over
    {
        let now = core::now_ms() as i64;
        let range = window.and_then(|w| crate::maintain::range_for(w, now, tz));
        let git_since = core::rfc3339(window.unwrap_or(0).max(0) as u64);
        let branch = crate::repo()
            .map(|r| crate::maintain::default_branch(&r.root))
            .unwrap_or_else(|_| "main".into());
        let day = core::stats::window_str(window);
        s.provenance = core::stats::provenance(range.as_deref(), &git_since, &branch, &day);
    }
    if let Ok(r) = crate::repo() {
        let (log, held) = super::lifecycle::marked(&r);
        s.lifecycle = core::stats::lifecycle(&log, &held);
        s.value.decision_by_state =
            core::stats::decision_by_state(&u.parsed, &log, &held, crate::write::cap::MAX_LIVE);
    }
    s
}

pub fn stats(a: &crate::Args) -> Result<(), String> {
    let (json, rows, day_view) = (a.has("json"), a.has("rows"), a.has("day"));
    // text selection: the short default, `--all` (today's full page), or one
    // section (`fael stats friction`). `--json` is already the full machine
    // shape, `--day`/`--days`/`--misses` are their own views — combining them
    // with a text selection rejects instead of guessing.
    let all = a.has("all");
    let section = a.pos.get(1).cloned();
    if all && section.is_some() {
        return Err(
            "rejected: --all and a section pick one — try 'fael stats --all' or 'fael stats friction'"
                .into(),
        );
    }
    if let Some(sec) = &section
        && !super::stats_select::SECTIONS.contains(&sec.as_str())
    {
        return Err(format!(
            "rejected: unknown stats section {sec:?} — try {} (or --all)",
            super::stats_select::SECTIONS.join(", ")
        ));
    }
    let session = a.one("session");
    if session.is_some()
        && (all || section.is_some() || day_view || a.has("days") || a.has("misses"))
    {
        return Err(
            "rejected: --session is its own view — drop --all/section/--day/--days/--misses".into(),
        );
    }
    if (all || section.is_some()) && (json || day_view || a.has("days") || a.has("misses")) {
        return Err(
            "rejected: --all and sections are text-only — drop --json/--day/--days/--misses".into(),
        );
    }
    // `--days --json`: the per-day series alone, from the parse — no repo logs,
    // git or gh (issue 01M5S63N follow-up: a board refresh paid ~4 s for it)
    let since = crate::report::since(a)?;
    if a.has("days") && json {
        let u = load_where(since, &|_| false);
        let days = core::stats::days(&u.parsed);
        println!("{}", serde_json::json!({ "days": days }));
        return Ok(());
    }
    // Default window: live raw + closed-month summary for additive counters;
    // repeat/cross/handoff stay live-only, distinct counts listed, not merged.
    let (raw_text, closed) = super::usage_retain::default_window(since);
    let u = load_text(&raw_text, since, &|_| true);
    if let Some(q) = session {
        return session_view(&q, json, &u);
    }
    if day_view {
        return day(json, &u);
    }
    // empty text searches ride after the page — even an "nothing yet" one,
    // since a repo that only searched has misses and no injections
    let misses = || {
        if let Some(l) = crate::find::misses::stats_line() {
            println!("{l}");
        }
    };
    let mut sums = super::usage_retain::summaries(since);
    // Past windows with no raw left list their summaries; the default window
    // aggregates live raw and merges the closed month below, so its
    // empty-live case still shows merged totals, not summaries alone.
    if since.is_some() && !json && !sums.is_empty() && (u.empty || u.parsed.n == 0) {
        super::usage_retain::print(&sums);
        misses();
        return Ok(());
    }
    // without `--json` an empty log is one friendly line, not a table of zeros;
    // the outcome joins read log + git + gh, so day one still has them
    if (u.empty || u.parsed.n == 0) && !json {
        if u.empty {
            println!("fael: no usage recorded yet");
        } else {
            println!(
                "fael: no usage recorded yet ({} from temp repos skipped)",
                u.parsed.skipped
            );
        }
        let cfg = crate::repo().map(|r| r.cfg).unwrap_or_default();
        if let Some(line) = super::stats_outcome::outcome_line(&aggregate(&u, &cfg, false)) {
            println!("{line}");
        }
        misses();
        return Ok(());
    }
    let cfg = crate::repo().map(|r| r.cfg).unwrap_or_default();
    let mut stats = aggregate(&u, &cfg, rows);
    if let Some((m, v)) = closed {
        super::usage_retain::merge_closed(&mut stats, &v);
        sums.push((m, v));
    } else if since.is_none() {
        // No summary to merge this run (the archive raw was read instead):
        // write it now so the next run skips the raw.
        super::usage_retain::ensure_closed_summary();
    }
    if json {
        println!("{}", super::usage_retain::json(&stats, sums)?);
        return Ok(());
    }
    // `--rows` rides every selection: the per-row detail under the page.
    let is_section = section.is_some();
    match (all, section) {
        (true, _) => super::stats_select::print_all(&stats, &u.path, &cfg.lang_rows),
        (false, Some(sec)) => {
            super::stats_select::print_section(&stats, &u.path, &cfg.lang_rows, &sec)
        }
        (false, None) => super::stats_select::print_default(&stats, &u.path),
    }
    super::usage_retain::print(&sums);
    // the short default skips the misses count line; `--all` keeps it
    if all || is_section {
        misses();
    }
    Ok(())
}

/// `fael stats --session <stem|prefix> [--json]`: one session's row from the
/// parse, the logs, this clone's runs and the clone's PR list (`gh_prs`: newest 200, cached 600 s — it may run `gh` once when stale).
fn session_view(q: &str, json: bool, u: &Usage) -> Result<(), String> {
    let (mut list, _) = core::stats::sessions(&u.parsed, &u.logs, &super::flow::runs());
    if let Ok(r) = crate::repo() {
        core::stats::link_prs(
            &mut list,
            &crate::maintain::cached_prs(),
            &r.root.to_string_lossy(),
        );
    }
    for v in core::stats::find_session(&list, q)? {
        if json {
            println!("{}", serde_json::to_string(v).map_err(|e| e.to_string())?);
        } else {
            super::stats_sessions::session_lines(v)
                .iter()
                .for_each(|l| println!("{l}"));
        }
    }
    Ok(())
}

/// One local day out of usage + logs (`fael stats --day [--json]`): the same
/// `DayView` struct the desktop popover reads over Tauri IPC, printed here
/// so the numbers can be checked against real data before the app exists.
fn day(json: bool, u: &Usage) -> Result<(), String> {
    let me = crate::repo().ok().map(|r| crate::writer(&r));
    let view = core::stats::day(
        &u.parsed,
        &u.logs,
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
pub(crate) fn local_tz_offset_min() -> i32 {
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
