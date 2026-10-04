//! The read/edit push: resolve renames through the L1 cache only (no git
//! spawn on this path), record the edit, cap the push at `budget.push_rows`,
//! and say each row once per session.

use super::asks::hook_meta;
use super::changed::{Ask, Blobs, edit_hint, read_seen, split_said};
use super::protocol::{Event, ctx};
use super::say::{Kind, Line, Outbox, Reply};
use super::state::{edits_path, lock_seen, record_edits, seen_path, take_hint, take_risk};
use super::usage::{memory_line, record_usage_shadow};
use crate::{aliases, core};
use std::collections::HashSet;

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
/// row cap — dropping render's budget cut line for the count lines above
/// (render itself is untouched, so find keeps its own cut line). Returns the
/// tagged rows, how many rows were actually said (usage counts only those),
/// and render's `bodies:` line, said on its own.
fn cut_body(
    body: String,
    sel: &core::Selection,
    files: &[String],
    tags: &crate::find::branches::BranchMap,
) -> (String, usize, Option<String>) {
    let n = body.lines().filter(|l| l.starts_with("- [")).count();
    let mut row = 0; // render keeps `sel.shown` order: the k-th row line is shown[k]
    let mut bodies = None;
    let mut out = String::new();
    for l in body.lines().filter(|l| !l.starts_with("… +")) {
        if l.starts_with("bodies: ") {
            bodies = Some(format!("{l}\n"));
            continue;
        }
        let l = super::also::drop_touched(l, files);
        let l = if l.starts_with("- [") {
            row += 1;
            super::also::label(&l, sel.tier(row - 1))
        } else {
            l
        };
        out.push_str(&l);
        out.push('\n');
    }
    (crate::find::branches::tag(out, tags), n, bodies)
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

/// The pushed paths as repo-relative files. Outside the repo falls away,
/// never errors out.
fn repo_files(c: &super::protocol::Ctx, raw: &[String]) -> Vec<String> {
    let mut files = vec![];
    for f in raw {
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
    files
}

pub(crate) fn push(e: &Event, event: &str) -> Reply {
    let no = Reply::default;
    let c = match ctx(e) {
        Some(c) => c,
        None => return no(),
    };
    // a shell call that wrote a file is an edit; only its usage label differs
    let edit = event == "edit" || event == super::search::SHELL_EDIT;
    let files = repo_files(&c, &e.files);
    if files.is_empty() {
        return no();
    }
    // only adopted repos — `add` derives files from this list only there
    if edit && !c.session.is_empty() && crate::journal::home(&c.repo).is_some() {
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
    let al = aliases::load(&c.repo, &c.log, false);
    let mut tiered = core::push_tiered(&c.log, &files, &al, !edit);
    // an edit is where a row goes stale: the agent is changing the code the
    // row describes, with both in front of it — the one moment to retire it.
    // The candidates are the tier-0 rows (an issue about a neighbouring file
    // is not this edit's to close) taken before the seen filter: a Read before
    // the Edit already said the row, yet the edit still offers its retire.
    let t0: Vec<(&core::Row, usize)> = if edit { tiered.clone() } else { vec![] };
    // a row already pushed into this context window is still there — say it
    // once. The lock spans read → append, so a batch of parallel reads queues
    // up behind the first instead of each pushing the same row.
    let mut out = Outbox::open(
        (!c.session.is_empty())
            .then(|| lock_seen(&seen_path(&c.session, &c.agent, &c.repo.root)))
            .flatten(),
    );
    if edit {
        out.record_in_context(&c, &tiered);
    }
    let (told, hinted) = read_seen(out.seen());
    let old: HashSet<&str> = out.seen().lines().collect();
    tiered.retain(|(r, _)| !old.contains(r.id.as_str()));
    let ask = Ask {
        log: &c.log,
        root: &c.repo.root,
        al: &al,
        session: &c.session,
        told: &told,
        hinted: &hinted,
    };
    let focus = super::focus::current(&c.session, &c.repo.root, &c.log);
    let sel = core::select(tiered, &focus, &policy);
    let notes = take_stashed(&c, &files);
    let mut blobs = Blobs::new();
    let (body, n, bodies) = cut_body(
        core::render(&c.log, &sel.shown, policy.budget),
        &sel,
        &files,
        &c.tags,
    );
    // usage counts only what was actually said — ids cut off never reached
    // any context, so stats must not count them. Same said rows feed the
    // shadow split on a read (PLAN-fael-file-hash chunk 3: usage-line only).
    let (shown, shadow) = split_said(&sel, n, edit, &c.repo.root, &al, &mut blobs);
    // ponytail: every edit push with rows names a row once per session
    let said = &sel.shown[..n.min(sel.shown.len())];
    // a retire for a row already in context, or a stashed line, still gets
    // said with no rows to join
    let hint = edit
        .then(|| edit_hint(&ask, &t0, said, &mut blobs))
        .flatten();
    say_rows(
        &mut out,
        &sel,
        &files,
        (body, n, bodies),
        &shown,
        policy.budget,
    );
    if let Some(h) = hint {
        out.say(Line {
            kind: Kind::Ask { ids: h.spent },
            text: format!("{}\n", h.text),
            // every form of the hint offers it
            action: Some("fael close".into()),
        });
    }
    if let Some(n) = notes {
        out.say(Line::notice(format!("{n}\n")));
    }
    let mut r = out.reply();
    if let Some(context) = r.context() {
        let meta = hook_meta(&c, None, true);
        record_usage_shadow(
            &c.client,
            event,
            &c.repo.root,
            context,
            &shown,
            &meta,
            // no row said, nothing to split
            shadow.filter(|_| n > 0),
        );
    }
    r.notice = whisper(&c, said, &files);
    r
}

/// The rows under their header, then the `bodies:` line and the count
/// lines — each said once per session (01M42F5B). Only what fit the budget
/// was said; the cut rows may push on a later read. A hub file with no Now
/// row says its count line under the header alone (PUSH_HUB_ROWS), or
/// nothing once this session was told.
fn say_rows(
    out: &mut Outbox,
    sel: &core::Selection,
    files: &[String],
    (body, n, bodies): (String, usize, Option<String>),
    shown: &[String],
    budget: usize,
) {
    // the cut goes on top too: the count lines sit under the rows, past where a reader stops
    let more = match sel.hidden(n).total() {
        0 => String::new(),
        h => format!(" ({n} of {})", n + h),
    };
    let header = format!("fael mem for {}{more}:\n", files.join(", "));
    // with nothing selected or omitted there is no cut to name
    let lines = if sel.shown.is_empty() && sel.omitted == 0 {
        vec![]
    } else {
        counts(sel, n, files)
    };
    let mut count = Line {
        kind: Kind::Count {
            files: files.join(","),
        },
        text: lines.iter().map(|l| format!("{l}\n")).collect(),
        action: Some("fael find --".into()),
    };
    if n > 0 {
        let usage = memory_line(&body, budget).unwrap_or_default();
        out.say(Line {
            kind: Kind::Row {
                ids: shown.to_vec(),
            },
            text: format!("{header}{body}{usage}"),
            action: None,
        });
    } else if !count.text.is_empty() {
        count.text = format!("{header}{}", count.text);
    }
    if let Some(text) = bodies {
        out.say(Line {
            kind: Kind::Bodies,
            text,
            action: Some("fael find <id>".into()),
        });
    }
    out.say(count);
}

/// PLAN-fael-visible-secretary chunk 4: the user hears which decision or
/// issue the agent was just reminded of — one line, the first such row, at
/// most once per file per session. Every reminded id also goes to the
/// tally for the turn's receipt. Notes and repo kinds stay quiet: a
/// reminder is a choice made or a problem known, never a row count.
fn whisper(c: &super::protocol::Ctx, said: &[&core::Row], files: &[String]) -> Option<String> {
    if !c.repo.cfg.notify_user {
        return None;
    }
    let hits: Vec<&core::Row> = said
        .iter()
        .copied()
        .filter(|r| matches!(r.kind.as_str(), "decision" | "issue"))
        .collect();
    let ids: Vec<&str> = hits.iter().map(|r| r.id.as_str()).collect();
    super::tally::note(&c.session, &c.repo.root, "reminded", &ids);
    let first = hits.first()?;
    if !super::tally::first_whisper(&c.session, &c.repo.root, files) {
        return None;
    }
    let label = match &first.key {
        Some(k) => format!("#{k}"),
        None => core::abbrev(&c.log).short(&first.id).to_string(),
    };
    let more = match hits.len() {
        1 => String::new(),
        n => format!(" +{} more", n - 1),
    };
    Some(format!(
        "fael: reminded agent — {label} \"{}\" ({}){more}",
        first.display_title(),
        files.join(", ")
    ))
}
