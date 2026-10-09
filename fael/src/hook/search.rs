//! The files a search or a shell read touched — the push for agents that read
//! through search tools or the shell instead of `Read` (fresh agents made 0
//! `Read` calls in 15 sessions). Nothing is guessed: a path counts only when
//! it names a file that exists, and only a reader command (`cat`, `sed`,
//! `grep`, `git show` …) makes its arguments a touch; a grep or glob hit list
//! counts only when it names one file. Tool names match
//! case-insensitively: Claude sends `Grep`/`Bash`/`Glob`, OpenCode `grep`/
//! `bash`/`glob`, and Codex shell calls arrive as `Bash` (unified exec included).
//! A shell call that wrote a file it names (`sed -i`, `python`, `> f`) pushes
//! that file as an edit — agents that edit through the shell get the stale-row
//! hint and the edit record like an `Edit` call does.

use super::cited::commit_cites;
use super::protocol::{Event, Reply, ctx};
use super::push::push;
use serde_json::Value;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// The most files one call pushes.
const MAX_FILES: usize = 8;
/// Output lines scanned for hit-list paths (each costs one `stat`).
const MAX_LINES: usize = 200;
const READERS: [&str; 12] = [
    "cat", "head", "tail", "sed", "awk", "nl", "less", "more", "bat", "grep", "egrep", "rg",
];
const GIT_READERS: [&str; 5] = ["show", "diff", "log", "blame", "grep"];
/// Shell tool names beyond `Bash` (Codex unified exec and OpenCode aliases).
const SHELLS: [&str; 5] = ["bash", "shell", "exec", "exec_command", "shell_command"];
/// An mtime this close to the hook counts as the call's own write.
// ponytail: fixed window, no start stamp — a file the Edit tool wrote under
// 3 s before a shell read of it reads as a shell edit too (the seen list
// keeps its rows from saying twice); a PreToolUse stamp if that shows up
const EDIT_WINDOW: Duration = Duration::from_secs(3);
/// Usage label of a shell edit push, so `fael stats` splits it from `edit`.
pub(crate) const SHELL_EDIT: &str = "shell-edit";
/// Hit-list fields of a search response (Claude `filenames`, shell `stdout`,
/// Codex `output`).
const HIT_KEYS: [&str; 5] = ["filenames", "stdout", "content", "output", "result"];

/// Data and generated files: a search that merely matched inside one reveals
/// no intent about it. Only the hit list is filtered — a file the call names
/// (`cat package.json`, a Grep `path`) still counts.
// ponytail: extension list, not a gitignore walk — add an extension when a noisy one shows up
fn is_data(p: &str) -> bool {
    const EXT: [&str; 5] = ["json", "lock", "map", "svg", "csv"];
    Path::new(p)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXT.contains(&e.to_ascii_lowercase().as_str()))
}

/// Files touched by one search/shell call, and what made them a touch:
/// `reader-arg` (a path the call names), `hitlist` (the one file a grep's
/// output names) or `glob` (the same from a glob). `input` is `tool_input`,
/// `response` is `tool_response` (a string, or an object with `stdout`,
/// `content`, `output`, `result` or `filenames`).
pub(crate) fn touched(
    tool: &str,
    input: &Value,
    response: &Value,
    cwd: &Path,
) -> (Vec<String>, &'static str) {
    let is_file = |p: &str| !p.is_empty() && cwd.join(p).is_file();
    let mut out: Vec<String> = vec![];
    // true when `p` was new
    let mut add = |p: &str| {
        let new = out.len() < MAX_FILES && is_file(p) && !out.iter().any(|o| o == p);
        if new {
            out.push(p.to_string());
        }
        new
    };
    let tool = tool.to_ascii_lowercase();
    let mut listing = tool == "grep" || tool == "glob";
    if tool == "grep" || tool == "glob" {
        // Grep (`path`) and Glob (`path`, OpenCode also `filePath`): a pattern
        // alone never names a file, and `add` drops it when it is not one.
        if let Some(p) = input["path"]
            .as_str()
            .or_else(|| input["filePath"].as_str())
        {
            add(p);
        }
    } else if SHELLS.contains(&tool.as_str()) {
        for seg in input["command"]
            .as_str()
            .unwrap_or("")
            .split(['|', ';', '&', '\n'])
        {
            let words: Vec<&str> = seg.split_whitespace().collect();
            let (git, rest) = match words.split_first() {
                Some((&"git", r)) => (true, r),
                Some(_) => (false, &words[..]),
                None => continue,
            };
            let reader = match rest.split_first() {
                Some((c, _)) if !git => c.rsplit('/').next().is_some_and(|c| READERS.contains(&c)),
                Some((c, _)) => GIT_READERS.contains(c),
                None => false,
            };
            if !reader {
                continue;
            }
            listing |= rest
                .first()
                .is_some_and(|c| c.ends_with("grep") || c.ends_with("rg"));
            for w in rest.iter().skip(1).filter(|w| !w.starts_with('-')) {
                let w = w.trim_matches(['\'', '"']);
                // `git show HEAD:src/a.rs`
                add(w.rsplit_once(':').filter(|_| git).map_or(w, |(_, p)| p));
            }
        }
    }
    let mut from_hits = false;
    if listing {
        let mut lines: Vec<&str> = vec![];
        match response {
            Value::String(s) => lines.extend(s.lines()),
            // a bare list (`Glob` on some clients) is already file paths
            Value::Array(a) => lines.extend(a.iter().filter_map(Value::as_str)),
            r => {
                for k in HIT_KEYS {
                    match &r[k] {
                        Value::String(s) => lines.extend(s.lines()),
                        Value::Array(a) => lines.extend(a.iter().filter_map(Value::as_str)),
                        _ => {}
                    }
                }
            }
        }
        // a match is not intent: a hit list over many files pushes nothing
        // (100-280 tokens of rows on files the agent only matched in), one
        // that names a single file pushes it like a named one
        let mut hits: Vec<&str> = vec![];
        for l in lines.into_iter().take(MAX_LINES) {
            // `path:line:text`, or a bare path (`grep -l`, `rg -l`, files_with_matches)
            let p = l.split_once(':').map_or(l, |(p, _)| p).trim();
            if !is_data(p) && !hits.contains(&p) && is_file(p) {
                hits.push(p);
            }
            if hits.len() > 1 {
                break;
            }
        }
        if let [p] = hits[..] {
            from_hits = add(p);
        }
    }
    let trigger = match (from_hits, tool == "glob") {
        (false, _) => "reader-arg",
        (true, false) => "hitlist",
        (true, true) => "glob",
    };
    (out, trigger)
}

/// Files a shell call wrote: a word of the command (quotes, brackets and
/// redirects split it) that is a file modified within `EDIT_WINDOW`. A
/// script that writes a file it never names is missed — never guessed.
pub(crate) fn edited(tool: &str, input: &Value, cwd: &Path) -> Vec<String> {
    if !SHELLS.contains(&tool.to_ascii_lowercase().as_str()) {
        return vec![];
    }
    let now = SystemTime::now();
    let fresh = |p: &str| {
        std::fs::metadata(cwd.join(p)).is_ok_and(|m| {
            m.is_file()
                && m.modified()
                    .is_ok_and(|t| now.duration_since(t).map_or(true, |d| d <= EDIT_WINDOW))
        })
    };
    let mut out: Vec<String> = vec![];
    let words = input["command"]
        .as_str()
        .unwrap_or("")
        .split(|c: char| c.is_whitespace() || "'\"`()[]{},;|&<>=".contains(c));
    for w in words {
        if out.len() < MAX_FILES && !w.is_empty() && !out.iter().any(|o| o == w) && fresh(w) {
            out.push(w.to_string());
        }
    }
    out
}

/// The push for one search/shell call: written files as a `SHELL_EDIT`
/// (first, so its rows carry the stale hint), the files it only read as a
/// `search`. An empty side loads nothing.
pub(crate) fn push_call(e: &Event, tool: &str, input: &Value, response: &Value) -> Reply {
    let cwd = Path::new(e.cwd.as_deref().unwrap_or("."));
    let wrote = edited(tool, input, cwd);
    let (read, trigger) = touched(tool, input, response, cwd);
    let read: Vec<String> = read.into_iter().filter(|f| !wrote.contains(f)).collect();
    let mut out = Reply::default();
    for (files, event, trigger) in [(wrote, SHELL_EDIT, SHELL_EDIT), (read, "search", trigger)] {
        if files.is_empty() {
            continue;
        }
        let e = Event { files, ..e.clone() };
        // one line per beat: the edit side's reminder wins
        out = out.and(push(&e, event, trigger));
    }
    out.and(commit_reply(e, tool, input))
}

/// The fix commit line; `files` = what the session edited, else a
/// placeholder — `fael add` rejects a row with no `--files`.
fn fix_commit(files: &[String]) -> String {
    let files = if files.is_empty() {
        "<files the fix touched>".to_string()
    } else {
        files.join(",")
    };
    format!(
        "fael: this `fix:` commit names no fael row and this session closed none (a row id anywhere in the commit message names it) — keep what broke for the next agent: fael add issue \"<what broke>\" --files {files} --key <area:topic> then fael close --key <area:topic> \"{}\"\n",
        fael_core::stats::CLOSE_TEMPLATE
    )
}

/// A `git commit` naming open issues (PLAN-fael-agent-ergonomics chunk 5):
/// one line per cited id with its ready `fael close`, each id once per
/// session. A `fix:` commit naming no row in a session that closed none
/// (PLAN-fael-experience-loop chunk 5b): the add + close that keeps it, once
/// per session. Nothing to say = silence. Each line is its kind through the
/// `Outbox` like every other push line, with a usage row so `fael stats`
/// reads its yield.
fn commit_reply(e: &Event, tool: &str, input: &Value) -> Reply {
    let no = Reply::default;
    if !SHELLS.contains(&tool.to_ascii_lowercase().as_str()) {
        return no();
    }
    let cmd = input["command"].as_str().unwrap_or("");
    if !super::cited::is_commit(cmd) {
        return no();
    }
    let c = match ctx(e) {
        Some(c) => c,
        None => return no(),
    };
    let mut out = super::say::Outbox::open(
        (!c.session.is_empty())
            .then(|| {
                super::state::lock_seen(&super::state::seen_path(
                    &c.session,
                    &c.agent,
                    &c.repo.root,
                ))
            })
            .flatten(),
    );
    let fresh: Vec<String> = commit_cites(&c.log, input)
        .into_iter()
        .filter(|id| !out.has(&format!("~cited:{id}")))
        .collect();
    let text: String = fresh
        .iter()
        .map(|id| format!("fael: {id} cited in a commit — done? `fael close {id} \"<why>\"`\n"))
        .collect();
    out.say(super::say::Line {
        kind: super::say::Kind::Cited { ids: fresh.clone() },
        text,
    });
    if super::cited::fix_uncited(&c.log, input)
        && !super::tally::closed_any(&c.session, &c.repo.root)
    {
        out.say(super::say::Line {
            kind: super::say::Kind::FixCommit,
            text: fix_commit(&super::state::edited_files(&c.session, &c.repo.root, 3)),
        });
    }
    let r = out.reply();
    if let Some(context) = r.context() {
        let meta = super::asks::UsageMeta {
            said: r.said(),
            ..super::asks::hook_meta(&c, None, true)
        };
        super::usage::record_usage(&c.client, "search", &c.repo.root, context, &fresh, &meta);
    }
    r
}
