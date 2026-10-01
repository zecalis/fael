//! Usage accounting (SPEC §8): every injection into context, per machine —
//! never in git. `fael stats` reads it back through `fael-core::stats` (the
//! numbers live in core so the desktop app shares them) and renders here.

use super::asks::{self, UsageMeta};
use super::state::{head_branch, now_rfc3339, state_dir};
use super::stats_text::{print_day, print_text};
use crate::core;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    asks::append_row(usage_row(client, event, repo, text, ids, meta));
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
    if let Some(real) = meta.real
        && let Ok(t) = serde_json::to_value(real)
    {
        row["real_tokens"] = t;
    }
    row
}

/// The one line that says what memory cost this injection, only when rows
/// rode along: `memory: ~412/800 tokens · 3 rows`. An estimate — `est_tokens`
/// of the rendered row lines, the same ruler `render` cuts by — so it carries
/// the `~`; the real bill lives in `fael stats`.
pub(crate) fn memory_line(body: &str, budget: usize) -> Option<String> {
    let rows: Vec<&str> = body.lines().filter(|l| l.starts_with("- [")).collect();
    (!rows.is_empty()).then(|| {
        let used: usize = rows.iter().map(|l| core::est_tokens(l)).sum();
        let n = rows.len();
        format!(
            "memory: ~{used}/{budget} tokens · {n} {}\n",
            if n == 1 { "row" } else { "rows" }
        )
    })
}

/// The log a stats row's repo has now — the tree + journal union the hooks
/// read, so a `store = "local"` repo (journal only) is not read as empty.
/// A repo path that no longer resolves falls back to its tree.
fn repo_log(repo: &str) -> core::Log {
    crate::repo_at(Path::new(repo))
        .map(|r| crate::read(&r))
        .unwrap_or_else(|_| core::read(&Path::new(repo).join(".fael")))
}

/// What `fael stats` and `fael report` read: `usage.jsonl` cut to `since`,
/// parsed, plus the log of every repo it names — finding repos and reading
/// journals stays with the caller (CLI or app); core joins the loaded logs,
/// never the filesystem.
pub(crate) struct Usage {
    pub(crate) path: PathBuf,
    pub(crate) empty: bool,
    pub(crate) parsed: core::stats::Parsed,
    pub(crate) logs: HashMap<String, core::Log>,
}

pub(crate) fn load(since: Option<i64>) -> Usage {
    let path = state_dir().join("usage.jsonl");
    let mut s = std::fs::read_to_string(&path).unwrap_or_default();
    if let Some(ms) = since {
        s = core::stats::since(&s, ms);
    }
    // benchmark/test repos live in the OS temp dir (01M3CRR6A) — the boundary
    // rides in as a parameter, so core stays pure. `/tmp` too: on macOS the
    // OS temp dir is under /var/folders, but agent scratchpads use /tmp
    let tmp = [
        std::env::temp_dir(),
        std::env::temp_dir().canonicalize().unwrap_or_default(),
        PathBuf::from("/tmp"),
        Path::new("/tmp").canonicalize().unwrap_or_default(),
    ];
    let parsed = core::stats::parse(&s, &path, &tmp);
    let mut logs: HashMap<String, core::Log> = HashMap::new();
    for repo in parsed.repos() {
        logs.entry(repo.to_string())
            .or_insert_with(|| repo_log(repo));
    }
    Usage {
        path,
        empty: s.trim().is_empty(),
        parsed,
        logs,
    }
}

/// The `Stats` `fael stats --json` prints — `fael report` renders the same one.
pub(crate) fn aggregate(u: &Usage, cfg: &core::Config, rows: bool) -> core::stats::Stats {
    core::stats::aggregate(&u.parsed, &u.logs, cfg, asks::constants().into(), rows)
}

pub fn stats(json: bool, rows: bool, day_view: bool, since: Option<i64>) -> Result<(), String> {
    let u = load(since);
    if day_view {
        return day(json, &u);
    }
    // without `--json` an empty log is one friendly line, not a table of zeros
    if u.empty && !json {
        println!("fael: no usage recorded yet");
        return Ok(());
    }
    if u.parsed.n == 0 && !json {
        println!(
            "fael: no usage recorded yet ({} from temp repos skipped)",
            u.parsed.skipped
        );
        return Ok(());
    }
    let cfg = crate::repo().map(|r| r.cfg).unwrap_or_default();
    let stats = aggregate(&u, &cfg, rows);
    if json {
        println!(
            "{}",
            serde_json::to_string(&stats).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    print_text(&stats, &u.path, &cfg.lang_rows);
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
