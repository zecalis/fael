//! The stop event: block the turn when the session did work (edits after
//! the newest row, or commits when the edit hook saw nothing) but filed no
//! row — or when the assistant announced a bug with no issue row since.
//! A bare risk mention never blocks: it joins the work block, or is stashed
//! for the next push to show once.

use super::markers::{bug_signal_from_transcript, has_bug_marker};
use super::protocol::{Event, Reply, ctx};
use super::state::{
    branch_path, edits_path, file_birth_ms, head_branch, now_rfc3339, risk_path, session_edits,
    session_key, state_dir,
};
use super::usage::record_usage;
use crate::{core, git};
use std::path::{Path, PathBuf};

/// The stop event, plus the branch-drift warning (row-hygiene chunk 9): when
/// HEAD moved since session-start — another session checked out its own
/// branch in this same worktree — one line says so. Never blocks, never
/// fires without a session-start baseline.
pub(crate) fn stop(e: &Event) -> Reply {
    let mut r = stop_inner(e);
    if let Some(line) = drift_line(e) {
        if r.block {
            r.reason = Some(match r.reason.take() {
                Some(reason) => format!("{reason}\n{line}"),
                None => line,
            });
        } else {
            r.context = Some(match r.context.take() {
                // stop's only context is this line, but stay append-safe
                Some(context) => format!("{context}{line}\n"),
                None => format!("{line}\n"),
            });
        }
    }
    r
}

/// `Some(line)` when the session started on another branch than the one
/// checked out now. No session, no repo, no baseline file (older sessions),
/// or an unreadable HEAD = None, silently.
fn drift_line(e: &Event) -> Option<String> {
    // `stop_hook_active` = the client already ran this turn's stop hook and is
    // re-entering (e.g. after the non-blocking warning continued the turn) —
    // say it once, so a client that surfaces the line cannot loop on it
    if e.stop_active {
        return None;
    }
    let session = e.session.as_deref().filter(|s| !s.is_empty())?;
    let cwd = e
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let repo = crate::repo_at(&cwd).ok()?;
    let start = std::fs::read_to_string(branch_path(session, &repo.root))
        .ok()?
        .trim()
        .to_string();
    if start.is_empty() {
        return None;
    }
    let now = head_branch(&repo.root)?;
    (start != now).then(|| {
        format!(
            "fael: branch changed mid-session ({start} → {now}) — \
             verify the branch before push or gh pr create"
        )
    })
}

fn stop_inner(e: &Event) -> Reply {
    let no = || Reply {
        block: false,
        reason: None,
        context: None,
    };
    let c = match ctx(e) {
        Some(c) => c,
        None => return no(),
    };
    if e.stop_active {
        return no();
    }
    // no log anywhere under .fael/ = fael never adopted here — allow before
    // spending a git spawn or a transcript read (decide_stop agrees: !has_log
    // never blocks)
    let log_path = c.repo.fael.join("log");
    if !(log_path.is_dir() && walk_jsonl(&log_path).next().is_some()) {
        return no();
    }
    // session start: an RFC 3339 time, or a transcript file's birthtime.
    // Recency compares run at ms precision (`since_ms`) — whole seconds race
    // with rows filed just before the session start; the `since` string stays
    // second-precision for `git log --since`, which only parses that far.
    let (since, since_ms) = match e.session.as_deref() {
        // an RFC 3339 start time (neutral callers without a transcript)
        Some(s) => match core::ts_ms(s) {
            Some(ms) => (since_secs(ms), ms),
            None => match file_birth_ms(Path::new(s)) {
                // a transcript file — birthtime (fallback: mtime) is the start
                Some(ms) => (since_secs(ms as i64), ms as i64),
                None => return no(),
            },
        },
        None => return no(),
    };
    let root = &c.repo.root;
    // edits count only after the session's newest row — a row filed early
    // does not cover hours of work after it
    let last_row = core::last_row_ms(&c.log, since_ms);
    let recorded = session_edits(&edits_path(&c.session, root));
    let mut edits: Vec<String> = vec![];
    for (path, at, ..) in &recorded {
        if last_row.is_none_or(|r| *at > r) && !edits.contains(path) {
            edits.push(path.clone());
        }
    }
    // commits only when the edit hook saw nothing (e.g. edits via a shell) —
    // the one git spawn left on this path
    let commits: Vec<String> = if recorded.is_empty() {
        // this branch's own commits only: --no-merges drops the merge of the base,
        // `--not` drops what came in with it (a squash merge made on the remote
        // after the session started). ponytail: the base is origin's HEAD/main/
        // master; a `[x]` keeps each --glob literal, so a missing ref is skipped
        // instead of failing the log.
        git(
            root,
            &[
                "log",
                "--no-merges",
                "--since",
                &since,
                "--format=%h %s",
                "HEAD",
                "--not",
                "--glob=refs/remotes/origin/HEA[D]",
                "--glob=refs/remotes/origin/mai[n]",
                "--glob=refs/remotes/origin/maste[r]",
            ],
        )
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default()
    } else {
        vec![]
    };
    // bug rule: a marker in the turn text, or the transcript tail after the
    // latest user message — cleared only by an issue row at or after the
    // match, never by one filed before the words
    let (bug_signal, bug_row_since) = bug_state(e, &c.log, since_ms);
    let reason = core::decide_stop(&core::StopFacts {
        stop_active: false,
        edits,
        commits,
        new_row: last_row.is_some(),
        has_log: true,
        bug_signal: bug_signal.clone(),
        bug_row_since,
    });
    // a Weak signal with no work block never blocks — stash one line for the
    // next push in this session (shown once, then deleted), and let through
    let Some(reason) = reason else {
        if let Some(sig) = &bug_signal
            && !sig.strong
            && !bug_row_since
        {
            stash_risk(&c.session, root, &sig.marker);
        }
        return no();
    };
    // once per session per worktree — the second end lets through, as the
    // reason promises; keying on the phrase blocked again per new phrase.
    // A new row opens one more work block, for edits made after it.
    // Only the bug rule's own block takes the "bug" slot: a Weak mention (or a
    // Strong one already cleared by an issue) riding a work block must key as
    // work, or it would dedupe away a later real report (review finding).
    let kind = match &bug_signal {
        Some(sig) if sig.strong && !bug_row_since => "bug".to_string(),
        _ => format!("work:{}", last_row.unwrap_or(0)),
    };
    if stop_blocked_before(&c.session, &root.to_string_lossy(), &kind) {
        return no();
    }
    // the reason lands in context like any push; stats reads these back to
    // count how many blocks were followed by a row
    let event = if kind == "bug" {
        "stop-bug"
    } else {
        "stop-work"
    };
    record_usage(&c.client, event, root, &reason, &[]);
    Reply {
        block: true,
        reason: Some(reason),
        context: None,
    }
}

/// The turn's bug announcement, if any — free text, or the transcript tail
/// after the latest user message — with whether an issue row at or after the
/// match already clears it. An issue filed before the words never does.
fn bug_state(e: &Event, log: &core::Log, since_ms: i64) -> (Option<core::BugSignal>, bool) {
    let bug_signal: Option<core::BugSignal> = match (&e.text, e.session.as_deref()) {
        (Some(text), _) => has_bug_marker(text).map(|h| core::BugSignal {
            marker: h.marker,
            strong: h.strong,
            at_ms: since_ms,
        }),
        (None, Some(t)) if Path::new(t).is_file() => {
            bug_signal_from_transcript(Path::new(t), since_ms).map(|h| core::BugSignal {
                marker: h.marker,
                strong: h.strong,
                at_ms: h.at_ms,
            })
        }
        _ => None,
    };
    let match_ms = bug_signal.as_ref().map(|s| s.at_ms).unwrap_or(since_ms);
    let cleared = log
        .rows
        .iter()
        .any(|r| r.kind == "issue" && core::ts_ms(&r.ts).is_some_and(|ms| ms >= match_ms));
    (bug_signal, cleared)
}

/// Floor to whole seconds for `git log --since` — flooring can only include
/// a commit from the start second, never drop one.
fn since_secs(ms: i64) -> String {
    let s = core::rfc3339((ms.max(0) as u64) / 1000 * 1000);
    s.replacen(".000Z", "Z", 1)
}

/// Any `.jsonl` under `dir` — lazy on purpose: the stop hook's adopted-here
/// check exits on the first hit instead of walking + sorting the whole log
/// tree the way `core::collect_files` does.
fn walk_jsonl(dir: &Path) -> impl Iterator<Item = PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    std::iter::from_fn(move || {
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "jsonl") {
                    return Some(p);
                }
            }
        }
        None
    })
}

/// Stash a Weak risk line for the next push in this session — shown once,
/// then deleted. Empty session = no stash (no push would ever show it).
fn stash_risk(session: &str, worktree: &Path, marker: &str) {
    if session.is_empty() {
        return;
    }
    let path = risk_path(session, worktree);
    if path
        .parent()
        .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
    {
        let _ = std::fs::write(&path, format!("{marker}\n"));
    }
}

/// True when this session already blocked for this worktree + kind — else
/// record the block and return false. Empty session = no dedupe (block).
fn stop_blocked_before(session: &str, worktree: &str, kind: &str) -> bool {
    if session.is_empty() {
        return false;
    }
    let path = state_dir()
        .join("stop-block")
        .join(format!("{}.jsonl", session_key(session)));
    if let Ok(s) = std::fs::read_to_string(&path) {
        for line in s.lines() {
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue, // a torn line must not lose the rest
            };
            if v["worktree"] == *worktree && v["kind"] == *kind {
                return true;
            }
        }
    }
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        use std::io::Write;
        let row = serde_json::json!({
            "ts": now_rfc3339().unwrap_or_default(),
            "worktree": worktree, "kind": kind,
        });
        let mut f = match std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
        {
            Ok(f) => f,
            Err(_) => return false,
        };
        // seal a torn tail so the new row starts on its own line
        let seal = core::needs_seal(&mut f).unwrap_or(false);
        let _ = writeln!(f, "{}{row}", if seal { "\n" } else { "" });
    }
    false
}
