//! The edit push: resolve renames through the L1 cache only (no git spawn on
//! this path), record the edit, cap the push at `budget.push_rows`, and say
//! each row once per session. A push is an `edit` or a `shell-edit` (a shell
//! call that wrote a file); no client sends a read push.

use super::asks::{UsageMeta, hook_meta};
use super::changed::{self, Ask, Blobs, edit_hint, read_seen};
use super::decision;
use super::protocol::{Event, ctx};
use super::say::{Kind, Line, Outbox, Reply};
use super::say_card::At;
use super::spent_notes::{self, Count};
use super::state::{
    clear_stash, edited_files, edits_path, lock_seen, peek_stash, record_edits, seen_path,
};
use super::usage::record_usage;
use crate::{aliases, core};
use fael_core::is_anchor;
use std::collections::HashSet;

/// The stashed Weak-signal line: one line, shown on the next push only.
fn risk_line(marker: &str, files: &[String]) -> String {
    format!(
        "fael note: this session mentioned a possible problem (\"{marker}\") — file an issue if it holds up: fael add issue \"<what is at risk>\" --files {}",
        files.join(",")
    )
}

/// The stashed fix line (PLAN-fael-experience-loop chunk 5a): the agent said
/// it fixed a bug and filed nothing — the add + close that keeps it.
fn fixed_line(phrase: &str, files: &[String]) -> String {
    format!(
        "fael: this session said it fixed a bug (\"{phrase}\") with no issue filed or closed since — keep it for the next agent: fael add issue \"<what broke>\" --files {} --key <area:topic> now, cite `(fael:<id>)` in the fix commit, then after it fael close --key <area:topic> \"{}\"\n",
        files.join(","),
        fael_core::stats::CLOSE_TEMPLATE
    )
}

/// The lines stop stashed for this push — the Weak risk mention (a notice)
/// and the fix line, whether or not rows
/// join them. Only looked at: `push` clears them once they were said, so a
/// push with no budget left leaves them for the next. Stop stashed them for
/// the session's own thread — a sub-agent's push leaves them there.
fn stashed(c: &super::protocol::Ctx, files: &[String]) -> Vec<Line> {
    if c.session.is_empty() || !c.agent.is_empty() {
        return vec![];
    }
    let [risk, fixed] = peek_stash(&c.session, &c.repo.root);
    let notice = risk.map(|m| Line::notice(format!("{}\n", risk_line(&m, files))));
    // the fix is in what the session edited, not in the file this push is on
    let fixed = fixed.map(|p| {
        let edited = edited_files(&c.session, &c.repo.root, 3);
        Line {
            kind: Kind::Fixed,
            text: fixed_line(&p, if edited.is_empty() { files } else { &edited }),
        }
    });
    notice.into_iter().chain(fixed).collect()
}

/// Render the selected rows with the token budget — the hard cap after the
/// row cap — dropping render's budget cut line, the header names the cut
/// (render itself is untouched, so find keeps its own cut line). Returns the
/// tagged rows and how many rows were actually said (usage counts only those).
/// Render's `bodies:` pointer line is dropped: pulled by id 26 of 282 (9%),
/// decision `product:push-noise-bar`.
fn cut_body(
    body: String,
    sel: &core::Selection,
    files: &[String],
    tags: &crate::journal::BranchMap,
) -> (String, usize) {
    let n = body.lines().filter(|l| l.starts_with("- [")).count();
    let mut row = 0; // render keeps `sel.shown` order: the k-th row line is shown[k]
    let mut out = String::new();
    for l in body
        .lines()
        .filter(|l| !l.starts_with("… +") && !l.starts_with("bodies: "))
    {
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
    (crate::journal::tag(out, tags), n)
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
        let one = std::slice::from_ref(&f);
        if let Ok(mut n) = core::normalize_files(one, &c.repo.cwd, &c.repo.root)
            .or_else(|e| sibling(c, &f).map_or(Err(e), |r| core::normalize_files(one, &r, &r)))
        {
            files.append(&mut n);
        }
    }
    files
}

/// The root of the sibling worktree holding `f` when it shares this clone's
/// journal: a session that `cd`s into `git worktree add ../wt` reads and edits
/// the same repo files there.
fn sibling(c: &super::protocol::Ctx, f: &str) -> Option<std::path::PathBuf> {
    let common = |root: &std::path::Path| crate::journal::root(root)?.parent()?.canonicalize().ok();
    let mine = common(&c.repo.root)?;
    let root = std::path::Path::new(f)
        .ancestors()
        .skip(1)
        .find(|d| d.join(".git").exists())?;
    (common(root)? == mine).then(|| root.to_path_buf())
}

/// Only adopted repos record an edit — `add` derives files from this list only there.
fn note_edit(c: &super::protocol::Ctx, files: &[String]) {
    if !c.session.is_empty() && crate::journal::home(&c.repo).is_some() {
        record_edits(
            &edits_path(&c.session, &c.repo.root),
            &c.repo.root.to_string_lossy(),
            &c.session,
            files,
        );
    }
}

/// The push of one hook event, `At::Edit` or `At::ShellEdit` (a shell call that
/// wrote a file is an edit; only its usage label and its ask differ). Any
/// other event is no push: it says nothing.
pub(crate) fn push(e: &Event, at: At) -> Reply {
    let no = Reply::default;
    let Some(event) = at.push_label() else {
        return no();
    };
    let c = match ctx(e) {
        Some(c) => c,
        None => return no(),
    };
    let files = repo_files(&c, &e.files);
    if files.is_empty() {
        return no();
    }
    note_edit(&c, &files);
    // L1 gather (renames resolve through the L1 cache only), then L3/L4 rank
    // + select against the session Focus: one small file read, no git spawn
    // on this path (one spawn is ~9 ms against a 5 ms ceiling) — session-start
    // builds the Focus file and refreshes that cache once per session. No
    // session, no file, a bad file: `Focus::default()` is today's order,
    // capped.
    // edits keep the same-directory tier: a module decision matters most
    // while changing that module.
    let policy = core::PushPolicy {
        max_rows: c.repo.cfg.push_rows,
        budget: c.repo.cfg.push_tokens,
        background: core::PUSH_BACKGROUND,
    };
    let al = aliases::load(&c.repo, &c.log, false);
    let mut tiered = core::push_tiered(&c.log, &files, &al, false);
    // an edit is where a row goes stale: the agent is changing the code the
    // row describes, with both in front of it — the one moment to retire it.
    // The candidates are the tier-0 rows (an issue about a neighbouring file
    // is not this edit's to close) taken before the seen filter: a row said at an
    // earlier push still gets its retire offered at the edit.
    // The ask is said at an edit only (6% earned at a shell edit, 56% at an
    // edit): a shell edit has no candidates, so none is built or recorded.
    let t0: Vec<(&core::Row, usize)> = if at == At::Edit {
        tiered.clone()
    } else {
        vec![]
    };
    // decision 01M5528X: a dormant row is never pushed — t0 above keeps the one
    // ask an edit earns. ponytail: hashes each candidate's files, short-circuits
    // on the first match; cache per repo if the 5 ms push ceiling says so.
    // Files over PUSH_MAX_BYTES have no verdict here while the pull side reads
    // up to the stamp cap, so a row the pull side calls dormant can still show
    // on push — the safe side (one extra row, never a wrong hide), and rare.
    let mut blobs = Blobs::new();
    // retired notes (chunk done) likewise; the merged-branch proof spawns git, so not here
    let share = c.repo.journal.as_deref().unwrap_or(&c.repo.root);
    let sid = super::state::session_id(&c.session);
    let spent = Count::load(share);
    let retired = hidden_notes(&c, &spent, sid);
    tiered.retain(|(r, _)| {
        !retired.contains(&r.id)
            && !changed::dormant(r, &c.repo.root, &al, &mut blobs, changed::PUSH_MAX_BYTES)
    });
    // a row already pushed into this context window is still there — say it
    // once. The lock spans read → append, so a batch of parallel reads queues
    // up behind the first instead of each pushing the same row.
    let mut out = Outbox::open(
        at,
        (!c.session.is_empty())
            .then(|| lock_seen(&seen_path(&c.session, &c.agent, &c.repo.root)))
            .flatten(),
    );
    out.record_in_context(&c, &tiered, &files);
    let (told, hinted) = read_seen(out.seen());
    let focus = super::focus::current(&c.session, &c.repo.root, &c.log);
    // a told File row still makes its file a hub (issue push:hub-files)
    let said = tiered
        .iter()
        .filter(|(r, t)| out.has(&r.id) && core::bucket(r, *t, &focus) == core::Bucket::File)
        .count();
    tiered.retain(|(r, _)| !out.has(&r.id));
    let ask = Ask {
        log: &c.log,
        root: &c.repo.root,
        share,
        files: &files,
        al: &al,
        session: &c.session,
        told: &told,
        hinted: &hinted,
    };
    let sel = hub_select(tiered, said, &focus, &policy, &mut out, &files);
    let notes = stashed(&c, &files);
    let has_notes = !notes.is_empty();
    let (body, n) = cut_body(
        core::render(&c.log, &sel.shown, policy.budget),
        &sel,
        &files,
        &c.tags,
    );
    // usage counts only what was actually said — ids cut off never reached
    // any context, so stats must not count them
    let shown: Vec<String> = sel.shown.iter().take(n).map(|r| r.id.clone()).collect();
    // ponytail: every edit push with rows names a row once per session
    let said = &sel.shown[..n.min(sel.shown.len())];
    // a retire for a row already in context, or a stashed line, still gets
    // said with no rows to join
    // t0 is empty at a shell edit, so it gets no hint
    let hint = edit_hint(&ask, &t0, said, &mut blobs);
    let mut lines = row_lines(&sel, &files, (body, n), &shown);
    lines.extend(hint.map(|h| Line {
        kind: Kind::Ask { ids: h.spent },
        text: format!("{}\n", h.text),
    }));
    lines.extend(retired_lines(&c, &spent, sid, &shown));
    lines.extend(super::overlap::lines(&c, &files));
    lines.extend(super::drift::line(&c));
    lines.extend(notes);
    // the hint and the stashed notice share the budget the rows left
    if out.say_within(policy.budget, lines) && has_notes {
        clear_stash(&c.session, &c.repo.root);
    }
    let mut r = out.reply();
    // remember the state of the issues actually asked (a budget-cut ask is not)
    super::asked::record(&c.log, share, &asked_rows(&t0, &r));
    if let Some(context) = r.context() {
        spent_notes::record(&c.log, share, sid, &note_ids(&c.log, &shown));
        let decision = decision::record(event, &sel, n, &focus);
        let meta = UsageMeta {
            said: r.said(),
            files: &files,
            decision: Some(&decision),
            ..hook_meta(&c, None, true)
        };
        record_usage(&c.client, event, &c.repo.root, context, &shown, &meta);
    }
    r.notice = super::tally::whisper(&c, said, &files);
    r
}

/// Notes the push skips: retired by chunk done (no git here), or said in five sessions.
fn hidden_notes(c: &super::protocol::Ctx, spent: &Count, sid: &str) -> HashSet<String> {
    let mut out = super::retired::ids(&c.repo, &c.log, false);
    out.extend(spent.retired(sid));
    out
}

/// The tier-0 issues whose edit ask was actually said.
fn asked_rows<'a>(t0: &[(&'a core::Row, usize)], r: &Reply) -> Vec<&'a core::Row> {
    let said = |row: &&core::Row| r.said().iter().any(|s| s.ask_id() == Some(&row.id));
    t0.iter().map(|(row, _)| *row).filter(said).collect()
}

/// The notes among the rows said.
fn note_ids<'a>(log: &core::Log, shown: &'a [String]) -> Vec<&'a str> {
    let is_note = |i: &&String| log.rows.iter().any(|r| r.kind == "note" && r.id == **i);
    shown.iter().filter(is_note).map(String::as_str).collect()
}

/// The one-time line of a note this session makes the last to be told it.
fn retired_lines(
    c: &super::protocol::Ctx,
    spent: &Count,
    sid: &str,
    shown: &[String],
) -> Vec<Line> {
    if sid.is_empty() {
        return vec![];
    }
    let abbrev = core::abbrev(&c.log);
    spent
        .crossing(sid, &note_ids(&c.log, shown))
        .into_iter()
        .filter_map(|id| {
            let row = c.log.rows.iter().find(|r| r.id == id)?;
            let short = abbrev.short(id);
            let file = row.files.first()?;
            Some(Line {
                kind: Kind::Retired { id: id.into() },
                text: format!(
                    "fael: note {short} retired from the push after {} sessions — a standing rule? re-file it: `fael add decision \"...\" --files {file} --supersedes {short}`\n",
                    spent_notes::SESSIONS
                ),
            })
        })
        .collect()
}

/// `select` with the session's hub record: a hub peeks once per file per
/// session — each re-read, or a search naming it beside another file, would
/// otherwise drip PUSH_HUB_PEEK more (issue push:hub-files). `said` = File
/// rows already told, still counted toward the hub.
fn hub_select<'a>(
    tiered: Vec<(&'a core::Row, usize)>,
    said: usize,
    focus: &core::Focus,
    policy: &core::PushPolicy,
    out: &mut Outbox,
    files: &[String],
) -> core::Selection<'a> {
    let peeked = |r: &core::Row| {
        let mut mine = r.files.iter().filter(|f| files.contains(f)).peekable();
        mine.peek().is_some() && mine.all(|f| out.has(&peek_key(f)))
    };
    let sel = core::select_with(
        tiered,
        focus,
        policy,
        &core::Prior {
            said,
            peeked: &peeked,
        },
    );
    for r in &sel.shown[sel.shown.len() - sel.peek..] {
        for f in r.files.iter().filter(|f| files.contains(f)) {
            out.spend(peek_key(f));
        }
    }
    sel
}

/// The seen-list mark of a hub file that had its one peek this session.
fn peek_key(file: &str) -> String {
    format!("~peek:{file}")
}

/// The lines of the rows under their header. Only
/// what fit the budget was said; the cut rows may push on a later read, and
/// the header names the cut. No row said, nothing said: the `… +N more —
/// fael find` count lines were dropped, 7 of 336 pulled (PLAN-fael-say-gate
/// chunk 4).
fn row_lines(
    sel: &core::Selection,
    files: &[String],
    (body, n): (String, usize),
    shown: &[String],
) -> Vec<Line> {
    if n == 0 {
        return vec![];
    }
    let more = match sel.hidden(n).total() {
        0 => String::new(),
        h => format!(" ({n} of {})", n + h),
    };
    let header = format!("fael mem for {}{more}:\n", files.join(", "));
    vec![Line {
        kind: Kind::Row {
            ids: shown.to_vec(),
        },
        text: format!("{header}{body}"),
    }]
}
