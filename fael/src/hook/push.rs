//! The read/edit push: resolve renames through the L1 cache only (no git
//! spawn on this path), record the edit, cap the push at `budget.push_rows`,
//! and say each row once per session.

use super::asks::{UsageMeta, hook_meta};
use super::changed::{Ask, Blobs, edit_hint, read_seen, split_said};
use super::check;
use super::decision::{self, Gate};
use super::protocol::{Event, ctx};
use super::say::{Kind, Line, Outbox, Reply};
use super::state::{
    clear_stash, edited_files, edits_path, lock_seen, peek_stash, read_turn, record_edits,
    seen_path, swap_touched, touched_path,
};
use super::usage::record_usage_shadow;
use crate::{aliases, core};

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
        "fael: this session said it fixed a bug (\"{phrase}\") with no issue filed or closed since — keep it for the next agent: fael add issue \"<what broke>\" --files {} --key <area:topic> now, then after the commit fael close --key <area:topic> \"{}\"\n",
        files.join(","),
        fael_core::stats::CLOSE_TEMPLATE
    )
}

/// The lines stop stashed for this push — the Weak risk mention and the
/// capture-reject hint (one notice), and the fix line, whether or not rows
/// join them. Only looked at: `push` clears them once they were said, so a
/// push with no budget left leaves them for the next. Stop stashed them for
/// the session's own thread — a sub-agent's push leaves them there.
fn stashed(c: &super::protocol::Ctx, files: &[String]) -> Vec<Line> {
    if c.session.is_empty() || !c.agent.is_empty() {
        return vec![];
    }
    let [risk, hint, fixed] = peek_stash(&c.session, &c.repo.root);
    let risk = risk.map(|m| risk_line(&m, files));
    let hint = hint.map(|h| format!("fael: {h}"));
    let notice: Vec<String> = risk.into_iter().chain(hint).collect();
    let notice = (!notice.is_empty()).then(|| Line::notice(format!("{}\n", notice.join("\n"))));
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

pub(crate) fn push(e: &Event, event: &str, trigger: &str) -> Reply {
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
    if edit {
        note_edit(&c, &files);
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
    )
    .in_turn(read_turn(&c.session, &c.repo.root));
    if edit {
        out.record_in_context(&c, &tiered, &files);
    }
    // the working set before this push, read under the same lock; no session,
    // no set — a row's `touch` is then unknown, not 0
    let touched = (!c.session.is_empty())
        .then(|| swap_touched(&touched_path(&c.session, &c.agent, &c.repo.root), &files));
    let (told, hinted) = read_seen(out.seen());
    let focus = super::focus::current(&c.session, &c.repo.root, &c.log);
    // a told File row still makes its file a hub (issue push:hub-files)
    let said = tiered
        .iter()
        .filter(|(r, t)| out.has(&r.id) && core::bucket(r, *t, &focus) == core::Bucket::File)
        .count();
    tiered.retain(|(r, _)| !out.has(&r.id));
    // the validation experiment (SPEC-fael-learn-loop §E): a candidate session's
    // search push loses the rows its gate cuts before `select` fills the cap
    let gate = Gate::for_push(&c, event, &mut tiered, touched.as_ref());
    let ask = Ask {
        log: &c.log,
        root: &c.repo.root,
        files: &files,
        al: &al,
        session: &c.session,
        told: &told,
        hinted: &hinted,
    };
    let sel = hub_select(tiered, said, &focus, &policy, &mut out, &files);
    let notes = stashed(&c, &files);
    let has_notes = !notes.is_empty();
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
    // t0 is empty off an edit, so a read or search gets no hint
    let hint = edit_hint(&ask, &t0, said, &mut blobs);
    let mut lines = row_lines(&sel, &files, (body, n, bodies), &shown);
    lines.extend(hint.map(|h| Line {
        kind: Kind::Ask { ids: h.spent },
        text: format!("{}\n", h.text),
    }));
    lines.extend(check::asks(&ask, &t0, said, edit));
    lines.extend(notes);
    // the hint and the stashed notice share the budget the rows left
    if out.say_within(policy.budget, lines) && has_notes {
        clear_stash(&c.session, &c.repo.root);
    }
    let mut r = out.reply();
    // a push the gate left silent is still a decision: its cut rows are what
    // `tune` measures. Nothing said, so nothing shown either.
    if r.context().is_some() || !gate.cut.is_empty() {
        let (context, n, shown) = match r.context() {
            Some(c) => (c, n, &shown[..]),
            None => ("", 0, &[][..]),
        };
        let decision = decision::record(trigger, &sel, n, &focus, touched.as_ref(), &gate);
        let meta = UsageMeta {
            said: r.said(),
            files: &files,
            decision: Some(&decision),
            ..hook_meta(&c, None, true)
        };
        record_usage_shadow(
            &c.client,
            event,
            &c.repo.root,
            context,
            shown,
            &meta,
            // no row said, nothing to split
            shadow.filter(|_| n > 0),
        );
    }
    r.notice = super::tally::whisper(&c, said, &files);
    r
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

/// The lines of the rows under their header, then the `bodies:` line. Only
/// what fit the budget was said; the cut rows may push on a later read, and
/// the header names the cut. No row said, nothing said: the `… +N more —
/// fael find` count lines were dropped, 7 of 336 pulled (PLAN-fael-say-gate
/// chunk 4).
fn row_lines(
    sel: &core::Selection,
    files: &[String],
    (body, n, bodies): (String, usize, Option<String>),
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
    let mut out = vec![Line {
        kind: Kind::Row {
            ids: shown.to_vec(),
        },
        text: format!("{header}{body}"),
    }];
    out.extend(bodies.map(|text| Line {
        kind: Kind::Bodies,
        text,
    }));
    out
}
