//! Usage accounting (SPEC §8): every injection into context, per machine —
//! never in git. `fael stats` reads it back through `fael-core::stats` (the
//! numbers live in core so the desktop app shares them) and renders here.

use super::asks::{self, UsageMeta};
use super::state::{head_branch, now_rfc3339};
use super::stats_text::{print_day, print_text};
use crate::core;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    record_usage_shadow(client, event, repo, text, ids, meta, None);
}

/// The read push's shadow verdicts (PLAN-fael-file-hash chunk 3): the shown
/// rows whose files changed since the row was written, and the shown rows
/// whose files all still match. Usage-line only — `text` (the rendered
/// context) is byte-identical with or without it, so `stats` counts and the
/// chunk-4 gate read the same rows either way. `None` when there is no verdict
/// to record: nothing was said, or an edit push (the hook runs after the
/// write, so every file the agent just edited would read as changed — the
/// label chunk 4 weighs sits on reads). `Some` always writes both keys,
/// possibly empty, so the gate can count "each side ≥ 30" straight from JSON;
/// `unchanged` is not `ids` minus `changed` — a row with no verdict is in
/// neither.
pub(crate) fn record_usage_shadow(
    client: &str,
    event: &str,
    repo: &Path,
    text: &str,
    ids: &[String],
    meta: &UsageMeta,
    shadow: Option<(Vec<String>, Vec<String>)>,
) {
    let mut row = usage_row(client, event, repo, text, ids, meta);
    if let Some((changed, unchanged)) = shadow {
        row["changed"] = changed.into();
        row["unchanged"] = unchanged.into();
    }
    asks::append_row(row);
}

/// A pull that showed rows (`find`, `mcp-find`, `kickoff`; PLAN-fael-say-gate
/// chunk 3): the outcome a Pointer, Count or Bodies line earns on. The shown
/// ids go under `found`, never `ids` (those count as pushes), and stats keeps
/// the line out of the injection totals. The query's key, files and id only —
/// free text is never stored (`find-misses.jsonl` holds the misses).
pub(crate) fn record_found(
    client: &str,
    event: &str,
    root: &Path,
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
    let mut row = usage_row(client, event, root, "", &[], &meta);
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

/// The log a stats row's repo has now — the tree + journal union the hooks
/// read, so a `store = "local"` repo (journal only) is not read as empty.
/// A repo path that no longer resolves falls back to its tree.
fn repo_log(repo: &str) -> core::Log {
    crate::repo_at(Path::new(repo))
        .map(|r| crate::read(&r))
        .unwrap_or_else(|_| core::fold_bumps(core::read(&Path::new(repo).join(".fael"))))
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
    load_where(since, &|_| true)
}

/// `load`, keeping only the lines (and logs) of repos `keep` accepts — a repo's
/// evaluator never reads another repo's usage (SPEC-fael-learn-loop §E).
pub(crate) fn load_where(since: Option<i64>, keep: &dyn Fn(&str) -> bool) -> Usage {
    load_text(super::usage_files::read(since), since, keep)
}

/// `load_where` over usage text the caller already read.
pub(crate) fn load_text(mut s: String, since: Option<i64>, keep: &dyn Fn(&str) -> bool) -> Usage {
    let path = super::usage_files::live();
    // `--since all` (0) cuts nothing: skip the second parse of every line
    if let Some(ms) = since.filter(|&ms| ms > 0) {
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
    let mut parsed = core::stats::parse(&s, &path, &tmp);
    parsed.kept.retain(|v| v["repo"].as_str().is_some_and(keep));
    let mut logs: HashMap<String, core::Log> = HashMap::new();
    // a removed worktree's path no longer leads to its journal: leave it out,
    // so stats read it as unknown, not as a log with no rows (01M4D7N4)
    for repo in parsed.repos().into_iter().filter(|r| keep(r)) {
        if Path::new(repo).exists() {
            logs.entry(repo.to_string())
                .or_insert_with(|| repo_log(repo));
        }
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
    let commits = u
        .logs
        .keys()
        .filter_map(|repo| Some((repo.clone(), commits(repo, *u.parsed.first_seen.get(repo)?))))
        .collect();
    let constants = asks::constants().into();
    core::stats::aggregate(&u.parsed, &u.logs, &commits, cfg, constants, rows)
}

/// The default branch's commits since the repo's first usage (H5 capture
/// recall) — one `git log` per repo, read by `fael stats`, never on a push.
/// No git, no branch: none.
fn commits(repo: &str, since_ms: i64) -> Vec<core::stats::Commit> {
    let root = Path::new(repo);
    let branch = crate::maintain::default_branch(root);
    let since = format!("--since={}", core::rfc3339(since_ms.max(0) as u64));
    let log = |r: &str| {
        crate::git(
            root,
            &["log", r, "--no-merges", &since, "--format=%H%x1f%B%x1e"],
        )
    };
    let out = log(&format!("origin/{branch}")).or_else(|| log(&branch));
    out.unwrap_or_default()
        .split('\x1e')
        .filter_map(|c| {
            let (sha, message) = c.trim().split_once('\x1f')?;
            Some(core::stats::Commit {
                sha: sha.to_string(),
                message: message.trim().to_string(),
            })
        })
        .collect()
}

pub fn stats(a: &crate::args::Args) -> Result<(), String> {
    let (json, rows, day_view) = (a.has("json"), a.has("rows"), a.has("day"));
    let u = load(crate::report::since(a)?);
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
    // without `--json` an empty log is one friendly line, not a table of zeros
    if u.empty && !json {
        println!("fael: no usage recorded yet");
        misses();
        return Ok(());
    }
    if u.parsed.n == 0 && !json {
        println!(
            "fael: no usage recorded yet ({} from temp repos skipped)",
            u.parsed.skipped
        );
        misses();
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
    misses();
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
