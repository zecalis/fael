//! Pure seam math: no db, no crew — verbs, paths, overlap, shared, rules.

use super::super::stats::state_dir;
use std::path::{Path, PathBuf};

/// The verbs `chunk` takes — what usage lines and the call line's `sub` name.
pub const CHUNK_VERBS: &[&str] = &[
    "add",
    "show",
    "start",
    "edit",
    "move",
    "note",
    "touch",
    "wait",
    "after",
    "unafter",
    "ask",
    "reply",
    "answer",
    "done",
    "accept",
    "drop",
    "park",
    "unpark",
    "apply",
    "approve",
    "unapprove",
];

/// The verbs `plan` takes — `show` is `export` by another name.
pub const PLAN_VERBS: &[&str] = &[
    "new", "export", "show", "apply", "edit", "park", "unpark", "done", "undone", "drop", "list",
    "lint",
];

/// Under the temp dir — canonical (`/private/var/…` on macOS) through the
/// nearest dir that exists yet.
pub fn in_tmp(p: &Path) -> bool {
    let p = p
        .ancestors()
        .find_map(|a| Some(a.canonicalize().ok()?.join(p.strip_prefix(a).ok()?)))
        .unwrap_or_else(|| p.to_path_buf());
    [std::env::temp_dir(), PathBuf::from("/tmp")]
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .any(|d| p.starts_with(d))
}

/// A repo under the temp dir belongs only on a registry under the temp dir.
pub fn is_scratch(root: &Path) -> bool {
    in_tmp(root) && !in_tmp(&state_dir())
}

/// The board's repos: `~/.local/state/fael/projects`, scratch pruned.
pub fn registered() -> Vec<PathBuf> {
    let text = std::fs::read_to_string(state_dir().join("projects")).unwrap_or_default();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .filter(|p| !is_scratch(p))
        .collect()
}

/// The main checkout, not this worktree: `<main>/.git/fael` is the journal.
pub fn main_root_for(root: &Path, journal: Option<&Path>) -> PathBuf {
    journal
        .and_then(Path::parent)
        .filter(|g| g.file_name().is_some_and(|n| n == ".git"))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf())
}

fn under(x: &str, y: &str) -> bool {
    let (x, y) = (x.trim_end_matches('/'), y.trim_end_matches('/'));
    x == y || x.strip_prefix(y).is_some_and(|r| r.starts_with('/'))
}

/// Two scopes overlap when a path of one is a prefix of the other's on a `/`
/// boundary (`src/` covers `src/a.rs`, `src/a` does not cover `src/ab.rs`).
pub fn overlap(a: &[String], b: &[String]) -> bool {
    a.iter()
        .any(|x| b.iter().any(|y| under(x, y) || under(y, x)))
}

fn scope_ok(paths: &[String]) -> Result<(), String> {
    for p in paths {
        let bad = p.is_empty()
            || p.starts_with('/')
            || p.contains(['\\', '*', '?', '[', ']', '\n'])
            || p.trim_end_matches('/')
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..");
        if bad {
            return Err(format!(
                "rejected: scope {p:?} — a repo-relative path prefix (`src/a.rs`, `docs/`), no `..`, no globs"
            ));
        }
    }
    Ok(())
}

/// The repo's `.fael/shared` prefixes; a line scope would reject is an error.
pub fn shared(root: &Path) -> Result<Vec<String>, String> {
    let path = root.join(".fael/shared");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect();
    scope_ok(&lines).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(lines)
}

/// `<state>/held/<session>`: the crew.db a session's `chunk start` bound a run in.
pub fn held_session_file(session: &str) -> Option<PathBuf> {
    session
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        .then(|| state_dir().join("held").join(session))
}

/// The db a session's `chunk start` bound, when the file names one that still exists.
pub fn held_session_db(session: &str) -> Option<PathBuf> {
    let m = held_session_file(session)?;
    let db = std::fs::read_to_string(m).ok()?;
    Path::new(&db).exists().then(|| PathBuf::from(db))
}

/// The chunk rules said once at the end of a session's first brief and kickoff: only what
/// applies to every chunk. Situational rules read at start are forgotten by the moment they
/// apply (issue 01M5Y6H4), so the verbs say them there; `rules_full` keeps them all.
pub fn rules(uid: &str, model: Option<&str>) -> String {
    format!("{}{}", rules_head(uid, model), rules_pointer(uid))
}

/// Core plus the situational rules: `chunk start --full`.
pub fn rules_full(uid: &str, model: Option<&str>) -> String {
    let (head, tail) = (rules_head(uid, model), rules_pointer(uid));
    format!("{head}{}{tail}", rules_situational())
}

fn rules_pointer(uid: &str) -> String {
    format!("- the rest (lane, wait format, plan sections): `fael chunk start {uid} --full`\n")
}

fn rules_head(uid: &str, model: Option<&str>) -> String {
    // crew decision 01M5FYK7: `mixed` = Opus settles the plan, Sonnet subagents do the plain edits
    let mixed = if model == Some("mixed") {
        "- model mixed: settle the plan yourself, hand the plain edits to small-model subagents (Claude Code: the Agent tool with `model: sonnet`), and review what they return before each step commit\n"
    } else {
        ""
    };
    format!(
        "## chunk rules\n\
         - state moves only through fael, never by editing a plan file:\n  \
         finished → from this worktree: `fael chunk done \"<result>\" --left none|<ids>` (PR found from the run's branch; `--pr N` to name it, or `--out <path>` for a file, image or video): done with a PR, review for a file the owner accepts; `-` as the text reads a long hand-off from stdin (`fael chunk done - --left none <<'EOF' … EOF`); after done, `fael chunk done {uid} \"<new>\"` (no `--pr`) amends the hand-off; the done text is the hand-off: first sentence = the outcome (what works or exists now), then what the owner checks\n  \
         need the owner → `fael chunk wait {uid} --on owner \"<question>\"` · need data → `--on data \"<what>\" --until YYYY-MM-DD`\n  \
         needs another chunk first → `fael chunk after {uid} <other> \"<why>\"`\n\
         - one chunk = one branch = one PR; never merge, the owner does\n\
         - via MCP, pass `cwd` (your working directory) on `chunk start` — it records your seat; done, wait and after find the run's seat themselves\n\
         - leftovers are rows, not PR prose: file each first (`fael add issue \"<one topic, no ; list>\" --title \"<≤15 words>\" --files <f>` or `fael chunk add \"<title>\" --plan <plan> --brief \"…\"`), then `fael chunk done` takes `--left none` or `--left <id|uid>,…` — done refuses without it; a number you measured goes on a hand-off line `measured: <what> · cmd: <shell line that prints it> · result: <printed>` (fael adds date:), which a chunk that lists this one in its `--left` prints in its brief; a word for a chunk not yet run → `fael chunk note <uid> \"<text>\"`; a fact the next session needs → `fael add note \"<state>\" --title \"<≤15 words>\" --files <f>`\n\
         {mixed}\
         - before done: a fresh session (a subagent, not you) checks your output — the diff, or the `--out` file — against this brief's Done and Scope, a Done criterion left undone is a major; fix what it finds and record its verdict with `--review clean|fixed|failed` (size S: check it yourself against Done; a review level this repo's own ship doc sets for the change, e.g. docs/SHIP.md, wins over this line — then omit `--review`)\n\
         - after each step that passes: a step commit (Conventional Commits, says what the step did, e.g. `feat(x): …`; never `WIP:`) + `fael chunk note {uid} \"<done; next>\"` — the checkpoint a resume reads\n\
         - the context was compacted, or the chunk will not finish this session? stop at the last passing step: a part that passes Done on its own ships as this chunk's PR, the rest `fael chunk add`ed and named in `--left`; otherwise a step commit + checkpoint note and end the session — the next `chunk start` resumes it\n"
    )
}

/// The rules the verbs say where they apply (lane start, wait), plus two with no verb
/// moment, kept for `--full` only.
fn rules_situational() -> String {
    format!(
        "- {LANE_LINE}\n\
         - {WAIT_OWNER}\n\
         - {WAIT_DATA}\n\
         - the work turns out to serve two lines of Done and the first already passes? ship it as this chunk's PR and `fael chunk add` the rest, or ask with `wait --on owner` — never grow one PR for both\n\
         - a section you read that your change makes untrue: update it in this chunk through the bill (`fael plan export <plan> --json`, edit its `sections`, `fael plan apply`) before done\n"
    )
}

/// Said by `chunk start --as lane` once it seats the lane (issue 01M5Y6H4).
pub const LANE_LINE: &str = "lane: one scope per worktree, own branch each — land in plan order, first-listed chunk first, as a suggestion only; the owner merges every PR, fael never enforces an order or guards git flow. End your run (`fael run end <R>`) before rebasing onto a lane that already landed, so its own diff is claimed first; `chunk done` and `run end` still say `outside declared scope` (+ `(overlaps open chunk …)`) as a fact, never a refusal";

const WAIT_OWNER: &str = "a wait --on owner is one question the owner answers in one line: options inline (`A or B?`), first line <= 100 chars, the action asked for — not preconditions that may be stale when it is read; then one clause per option on what it leads to, and end `Recommend: <option> because <evidence>` — you hold the most context now";

const WAIT_DATA: &str = "a wait --on data names what is measured, the command that measures it and the bar that ends the wait";

/// The format line `chunk wait` prints after it records: an owner question without a
/// `Recommend:`, or any data wait. A hint, never a refusal (issue 01M5Y6H4).
pub fn wait_hint(on: &str, text: &str) -> Option<String> {
    match on {
        "owner" if !text.contains("Recommend:") => Some(format!("hint: {WAIT_OWNER}")),
        "data" => Some(format!("hint: {WAIT_DATA}")),
        _ => None,
    }
}
