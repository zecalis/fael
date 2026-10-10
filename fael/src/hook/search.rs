//! The search/shell hook: a shell call that wrote a file it names (`sed -i`,
//! `python`, `> f`) pushes that file as an edit — agents that edit through
//! the shell get the stale-row hint and the edit record like an `Edit` call
//! does — and a `git commit` gets its cited-row and fix-commit lines. A search
//! or a shell read says nothing: rows reach the agent at the edit. Tool names
//! match case-insensitively: Claude sends `Grep`/`Bash`/`Glob`, OpenCode
//! `grep`/`bash`/`glob`, and Codex shell calls arrive as `Bash`.

use super::cited::commit_cites;
use super::protocol::{Event, Reply, ctx};
use super::push::push;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The most files one call pushes.
const MAX_FILES: usize = 8;
/// Shell tool names beyond `Bash` (Codex unified exec and OpenCode aliases).
const SHELLS: [&str; 5] = ["bash", "shell", "exec", "exec_command", "shell_command"];
/// An mtime this close to the hook counts as the call's own write.
// ponytail: fixed window, no start stamp — a file the Edit tool wrote under
// 3 s before a shell read of it reads as a shell edit too (the seen list
// keeps its rows from saying twice); a PreToolUse stamp if that shows up
const EDIT_WINDOW: Duration = Duration::from_secs(3);
/// Usage label of a shell edit push, so `fael stats` splits it from `edit`.
pub(crate) const SHELL_EDIT: &str = "shell-edit";

/// A shell command's segments, each with the directory it runs in: a `cd`
/// to an existing directory moves every later segment, so `cd ../wt && cat
/// a.rs` reads `../wt/a.rs`, not `a.rs` at the session's cwd.
// ponytail: no subshell scope — `(cd x && …); cat y` reads y in x too; the
// file still has to exist there
fn segments<'a>(cmd: &'a str, cwd: &Path) -> Vec<(PathBuf, &'a str)> {
    let mut dir = cwd.to_path_buf();
    cmd.split(['|', ';', '&', '\n'])
        .map(|seg| {
            let mut w = seg.split_whitespace();
            if w.next().map(|c| c.trim_start_matches('(')) == Some("cd")
                && let Some(d) = w
                    .next()
                    .map(|d| dir.join(d.trim_matches(['\'', '"', ')'])))
                    .filter(|d| d.is_dir())
            {
                dir = d;
            }
            (dir.clone(), seg)
        })
        .collect()
}

/// `p` as a hook names it: as written at the session's cwd, else under the
/// directory a `cd` moved to (absolute, so push maps it to its repo).
fn named(dir: &Path, cwd: &Path, p: &str) -> String {
    if dir == cwd {
        p.to_string()
    } else {
        dir.join(p).to_string_lossy().into_owned()
    }
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
    for (dir, seg) in segments(input["command"].as_str().unwrap_or(""), cwd) {
        let words = seg.split(|c: char| c.is_whitespace() || "'\"`()[]{},;|&<>=".contains(c));
        for w in words.filter(|w| !w.is_empty()).map(|w| named(&dir, cwd, w)) {
            if out.len() < MAX_FILES && !out.contains(&w) && fresh(&w) {
                out.push(w);
            }
        }
    }
    out
}

/// The push for one search/shell call: the files it wrote, as a `SHELL_EDIT`.
/// Files it only read say nothing — rows come at the edit.
pub(crate) fn push_call(e: &Event, tool: &str, input: &Value, _response: &Value) -> Reply {
    let cwd = Path::new(e.cwd.as_deref().unwrap_or("."));
    let wrote = edited(tool, input, cwd);
    let out = if wrote.is_empty() {
        Reply::default()
    } else {
        push(
            &Event {
                files: wrote,
                ..e.clone()
            },
            SHELL_EDIT,
            SHELL_EDIT,
        )
    };
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
        "fael: this `fix:` commit names no fael row and this session closed none — keep what broke for the next agent: fael add issue \"<what broke>\" --files {files} --key <area:topic>, cite `(fael:<id>)` in a commit on this branch (a squash keeps it), then fael close --key <area:topic> \"{}\"\n",
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
    let ab = crate::core::abbrev(&c.log);
    let text: String = fresh
        .iter()
        .map(|id| {
            let id = ab.short(id);
            format!("fael: {id} cited in a commit — `(fael:{id})` in its message links the fix on main; done? `fael close {id} \"<why>\"`\n")
        })
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
