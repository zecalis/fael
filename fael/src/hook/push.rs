//! The read/edit push: resolve renames through the L1 cache only (no git
//! spawn on this path), record the edit, cap the push at `budget.push_rows`,
//! and say each row once per session.

use super::asks::hook_meta;
use super::protocol::{Event, Reply, ctx};
use super::state::{edits_path, lock_seen, record_edits, seen_path, take_hint, take_risk};
use super::usage::record_usage;
use crate::{aliases, core};
use std::collections::HashSet;
use std::io::{Read, Write};

/// Said under the rows of an edit push (see `push`).
const STALE_HINT: &str = "fael: a row above the code now says or contradicts? `fael close <id> \"now in <file>\"` or re-file it with `--supersedes <id>`";

/// The stashed Weak-signal line: one line, shown on the next push only.
fn risk_line(marker: &str, files: &[String]) -> String {
    format!(
        "fael note: this session mentioned a possible problem (\"{marker}\") — file an issue if it holds up: fael add issue \"<what is at risk>\" --files {}",
        files.join(",")
    )
}

/// Take the lines stop stashed for this push — the Weak risk mention and the
/// capture-reject hint, each shown once, whether or not rows join them.
/// Joined, or `None` when there are none. Stop stashed them for the session's
/// own thread — a sub-agent's push leaves them there.
fn take_stashed(c: &super::protocol::Ctx, files: &[String]) -> Option<String> {
    if c.session.is_empty() || !c.agent.is_empty() {
        return None;
    }
    let risk = take_risk(&c.session, &c.repo.root).map(|m| risk_line(&m, files));
    let hint = take_hint(&c.session, &c.repo.root).map(|h| format!("fael: {h}"));
    let lines: Vec<String> = risk.into_iter().chain(hint).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The directory calls that reach the hidden same-dir ring: every distinct
/// parent of the queried files, each with a trailing `/` (a directory query).
/// `None` when no file sits in a directory (root-level files).
fn dirs_arg(files: &[String]) -> Option<String> {
    let mut dirs: Vec<&str> = files
        .iter()
        .filter_map(|f| f.rsplit_once('/').map(|(d, _)| d))
        .collect();
    dirs.sort_unstable();
    dirs.dedup();
    (!dirs.is_empty()).then(|| {
        dirs.iter()
            .map(|d| format!("{d}/"))
            .collect::<Vec<_>>()
            .join(",")
    })
}

/// The count lines under the rendered rows — one per class, each naming the
/// exact call that reaches it: tier-0 cuts by the file (the row cap and the
/// budget cut), the same-dir ring by the query's directory, and each hidden
/// key (folded into one line past the first). `rendered` is how many rows render actually said. `hidden` routes the
/// budget cut by each row's L1 tier too, so a budget-cut same-dir or
/// shared-key row (a Now row the cap never touched) names the right call.
fn counts(sel: &core::Selection, rendered: usize, files: &[String]) -> Vec<String> {
    let mut out = vec![];
    let hidden = sel.hidden(rendered);
    if hidden.file > 0 {
        let what = if files.len() == 1 {
            "this file"
        } else {
            "these files"
        };
        out.push(format!(
            "… +{} more about {what} — fael find --files {}",
            hidden.file,
            crate::find::quoted(&files.join(","))
        ));
    }
    if hidden.dirs > 0
        && let Some(dirs) = dirs_arg(files)
    {
        out.push(format!(
            "… +{} more in {dirs} — fael find --files {}",
            hidden.dirs,
            crate::find::quoted(&dirs)
        ));
    }
    match hidden.keys.as_slice() {
        [] => {}
        [(key, n)] => out.push(format!(
            "… +{n} more with #{key} — fael find --key {}",
            crate::find::quoted(key)
        )),
        // one line however many keys: a file whose rows carry a dozen keys
        // used to spend more tokens on the footer than on the rows
        many => {
            let mut top = many.to_vec();
            top.sort_by_key(|a| std::cmp::Reverse(a.1)); // stable: ties keep encounter order
            let named: Vec<String> = top
                .iter()
                .take(3)
                .map(|(k, n)| format!("#{k} ({n})"))
                .collect();
            let rest = match many.len() - named.len() {
                0 => String::new(),
                r => format!(", +{r} keys"),
            };
            out.push(format!(
                "… +{} more under {} keys: {}{rest} — fael find --key <key>",
                many.iter().map(|(_, n)| n).sum::<usize>(),
                many.len(),
                named.join(", ")
            ));
        }
    }
    out
}

/// Render the selected rows with the token budget — the hard cap after the
/// row cap — swapping render's budget cut line for the count lines above
/// (render itself is untouched, so find keeps its own cut line). Returns the
/// tagged body and how many rows were actually said (usage counts only those).
fn cut_body(
    body: String,
    sel: &core::Selection,
    files: &[String],
    tags: &crate::find::branches::BranchMap,
) -> (String, usize) {
    let n = body.lines().filter(|l| l.starts_with("- [")).count();
    let mut lines: Vec<String> = body
        .lines()
        .filter(|l| !l.starts_with("… +"))
        .map(str::to_string)
        .collect();
    lines.extend(counts(sel, n, files));
    (crate::find::branches::tag(lines.join("\n") + "\n", tags), n)
}

/// A `scheme:ref` anchor (opaque, never a filesystem path) — the same rule
/// core uses: a scheme of ≥ 2 lower chars before the first `:`.
pub(crate) fn is_anchor(f: &str) -> bool {
    if f.contains("://") || !f.contains(':') {
        return false;
    }
    // cheap anchor check without reaching into core: scheme of ≥2 lower chars
    let scheme = f.split(':').next().unwrap_or("");
    scheme.len() >= 2
        && scheme.as_bytes()[0].is_ascii_lowercase()
        && scheme
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'+' | b'.' | b'-'))
}

pub(crate) fn push(e: &Event, event: &str) -> Reply {
    let no = || Reply {
        block: false,
        reason: None,
        context: None,
    };
    let c = match ctx(e) {
        Some(c) => c,
        None => return no(),
    };
    // normalize through core — outside the repo falls away, never errors out
    let mut files = vec![];
    for f in &e.files {
        // the client sends whatever the OS gave it (`/var/…` vs `/private/var/…`);
        // resolve symlinks while the repo root is already resolved, or the
        // lexical strip in normalize_files reads the file as outside the repo
        // (anchors are opaque, never filesystem paths)
        let f = if is_anchor(f) {
            f.clone()
        } else {
            std::fs::canonicalize(c.repo.cwd.join(f)) // join keeps an absolute f
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| f.clone())
        };
        if let Ok(mut n) =
            core::normalize_files(std::slice::from_ref(&f), &c.repo.cwd, &c.repo.root)
        {
            files.append(&mut n);
        }
    }
    if files.is_empty() {
        return no();
    }
    // only adopted repos — stop never blocks without a log anyway
    if event == "edit" && !c.session.is_empty() && crate::journal::home(&c.repo).is_some() {
        record_edits(
            &edits_path(&c.session, &c.repo.root),
            &c.repo.root.to_string_lossy(),
            &c.session,
            &files,
        );
    }
    // L1 gather (renames resolve through the L1 cache only), then L3/L4 rank
    // + select against the session Focus: one small file read, no git spawn
    // on this path (one spawn is ~9 ms against a 5 ms ceiling) — session-start
    // builds the Focus file and refreshes that cache once per session. No
    // session, no file, a bad file: `Focus::default()` is today's order,
    // capped.
    // reads skip the same-directory tier (chunk 2: the noisiest tier — rows
    // about neighbouring files); edits keep it, a module decision matters
    // most while changing that module.
    let policy = core::PushPolicy {
        max_rows: c.repo.cfg.push_rows,
        budget: c.repo.cfg.push_tokens,
        background: core::PUSH_BACKGROUND,
    };
    let mut tiered = core::push_tiered(
        &c.log,
        &files,
        &aliases::load(&c.repo, &c.log, false),
        event != "edit",
    );
    // a row already pushed into this context window is still there — say it
    // once. The lock spans read → append, so a batch of parallel reads queues
    // up behind the first instead of each pushing the same row.
    let mut seen = (!c.session.is_empty())
        .then(|| lock_seen(&seen_path(&c.session, &c.agent, &c.repo.root)))
        .flatten();
    if let Some(f) = &mut seen {
        let mut old = String::new();
        let _ = f.read_to_string(&mut old);
        let old: HashSet<&str> = old.lines().collect();
        tiered.retain(|(r, _)| !old.contains(r.id.as_str()));
    }
    let focus = super::focus::read(&c.session, &c.repo.root);
    let sel = core::select(tiered, &focus, &policy);
    let notes = take_stashed(&c, &files);
    if sel.shown.is_empty() {
        // a stashed line still gets said, even with no rows to join
        if let Some(context) = notes {
            let meta = hook_meta(&c, None, true);
            record_usage(&c.client, event, &c.repo.root, &context, &[], &meta);
            return Reply {
                block: false,
                reason: None,
                context: Some(context),
            };
        }
        return no();
    }
    let (body, n) = cut_body(
        core::render(&c.log, &sel.shown, policy.budget),
        &sel,
        &files,
        &c.tags,
    );
    // usage counts only what was actually said — ids cut off never reached
    // any context, so stats must not count them
    let shown: Vec<String> = sel.shown.iter().take(n).map(|r| r.id.clone()).collect();
    if let Some(mut f) = seen {
        // only what fit the budget was said; the cut rows may push on a later read
        let out: String = shown.iter().map(|id| format!("{id}\n")).collect();
        let _ = f.write_all(out.as_bytes());
    }
    let context = format!("fael mem for {}:\n{body}", files.join(", "));
    // an edit is where a row goes stale: the agent is changing the code the
    // row describes, with both in front of it — the one moment to retire it.
    // ponytail: every edit push with rows; once per session if it costs too much
    let context = if event == "edit" {
        format!("{context}{STALE_HINT}\n")
    } else {
        context
    };
    let context = match notes {
        Some(n) => format!("{context}\n{n}"),
        None => context,
    };
    let meta = hook_meta(&c, None, true);
    record_usage(&c.client, event, &c.repo.root, &context, &shown, &meta);
    Reply {
        block: false,
        reason: None,
        context: Some(context),
    }
}
