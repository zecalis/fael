//! The session-start event (decision 01M5528X): hand-offs and the rows
//! assigned to the reader, nothing else — no count line, no due revisits, no
//! decisions. Nothing for the reader = nothing said. Plus the one-line
//! warning when the client wiring is behind.

use super::asks::{UsageMeta, hook_meta};
use super::focus;
use super::protocol::{Event, Reply, ctx};
use super::say::{Kind, Line, Outbox};
use super::state::{mark_started, prune_sessions, seen_path, state_dir};
use super::usage::record_usage;
use crate::{aliases, core};
use std::path::Path;

pub(crate) fn session_start(e: &Event) -> Reply {
    let no = Reply::default;
    let Some(mut c) = ctx(e) else { return no() };
    // decision 01M5528X: a dormant row is never in the session brief
    crate::hook::mark_dormant(&c.repo, &mut c.log);
    // a run killed mid-turn never reaches Stop: its session is kept from the first turn
    super::stop::seen(e);
    prune_sessions(&state_dir().join("sessions"));
    prune_sessions(&state_dir().join("held"));
    // one line, every start, fail-open: the Stop-block mode was removed
    // 2026-10-03 and its dir is garbage (issue 01M56WGH)
    let _ = std::fs::remove_dir_all(state_dir().join("stop-block"));
    mark_started(&c.session, &c.repo.root);
    // PLAN-fael-auto-update chunk 3: the daily self-update check, detached too
    crate::install::auto::start();
    // a compacted or cleared context lost the rows pushed into it — tell them again
    if matches!(e.source.as_deref(), Some("compact" | "clear")) && !c.session.is_empty() {
        let _ = std::fs::remove_file(seen_path(&c.session, "", &c.repo.root));
    }
    let branch = start_branch(&c.session, &c.repo.root);
    // once per session: pick up renames committed since the last session, so
    // the read/edit push (which never spawns git) resolves them
    aliases::load(&c.repo, &c.log, true);
    // reader identity needs git, which session-start already spawns; read/edit
    // never compute it (SPEC fail examples)
    let reader = crate::writer(&c.repo);
    let all: Vec<&core::Row> = core::find(&c.log, &core::Filter::default());
    let f = core::Focus::from_rows(branch.as_deref(), &all);
    focus::write(&c.session, &c.repo.root, &f);
    let shown = start_rows(&all, &f, &reader, e.client.as_deref());
    let mut body = crate::journal::tag(
        core::render(&c.log, &shown, c.repo.cfg.kickoff_tokens),
        &c.tags,
    );
    // like the read/edit push: usage counts only the ids render actually
    // said — rows the budget cut off never reached any context
    let n = body.lines().filter(|l| l.starts_with("- [")).count();
    let said = &shown[..n.min(shown.len())];
    let mut ids: Vec<String> = said.iter().map(|r| r.id.clone()).collect();
    let mut keys: Vec<String> = ids.iter().map(|i| format!("~start:{i}")).collect();
    if let Some((uid, line)) = chunk_line(&c.repo) {
        keys.push(format!("~start:chunk:{uid}"));
        body.push_str(&line);
    }
    let adopted = crate::journal::home(&c.repo).is_some();
    let seen = seen_path(&c.session, &c.agent, &c.repo.root);
    let mut out = Outbox::open(
        (!c.session.is_empty())
            .then(|| super::state::lock_seen(&seen))
            .flatten(),
    );
    let brief = Kind::Brief { keys };
    // a brief already said this context (a resume) names no ids, even when a notice is said
    if !out.fresh(&brief) {
        ids.clear();
    }
    out.say(Line {
        kind: brief,
        text: body,
    });
    for warn in (warnings(&c.repo, adopted).into_iter()).chain(crate::install::auto::notice()) {
        out.say(Line::notice(format!("{warn}\n")));
    }
    let mut r = out.reply();
    // the session just began — no round completed yet, so no real tokens. A
    // start that said nothing lands too, as `SILENT_START`: the say-gate
    // revert check divides by every session-start (chunk 4c), and stats keep
    // that event out of every injection total
    let (event, context) = match r.context() {
        Some(t) => ("session-start", t),
        None => {
            ids.clear();
            (core::stats::SILENT_START, "")
        }
    };
    let meta = UsageMeta {
        said: r.said(),
        ..hook_meta(&c, None, false)
    };
    record_usage(&c.client, event, &c.repo.root, context, &ids, &meta);
    if context.is_empty() {
        return no();
    }
    r.notice = brief_line(&c, said);
    r
}

/// The rows session start hands over, ranked: open hand-offs (`:handoff`
/// key) of this branch's work — filed on the start branch or carrying a
/// Focus key — then open rows `to` this reader. `client` is the agent's own
/// name (`opencode`, `codex`, `claude`): a row `--to opencode` reaches every
/// OpenCode session in the repo, whoever runs it.
fn start_rows<'a>(
    all: &[&'a core::Row],
    f: &core::Focus,
    reader: &str,
    client: Option<&str>,
) -> Vec<&'a core::Row> {
    let handoff = |r: &core::Row| {
        let key = r.key.as_deref().filter(|k| k.ends_with(":handoff"));
        key.is_some_and(|k| f.keys.contains(k))
            || (key.is_some() && r.branch().is_some() && r.branch() == f.branch.as_deref())
    };
    let mine = |r: &core::Row| {
        r.to_who().is_some_and(|t| {
            core::to_matches(t, reader) || client.is_some_and(|c| t.eq_ignore_ascii_case(c))
        })
    };
    let rank = |rows: Vec<&'a core::Row>| core::ranked(rows, Some(reader), |_| 0, core::fresh_ts);
    let mut out = rank(all.iter().copied().filter(|r| handoff(r)).collect());
    out.extend(rank(
        all.iter()
            .copied()
            .filter(|r| !handoff(r) && mine(r))
            .collect(),
    ));
    out
}

/// The live chunk run this worktree holds (`fael chunk start` here, maybe in
/// another session): its uid and the line that points at its brief. No
/// fael.db, no run = `None`.
fn chunk_line(repo: &crate::Repo) -> Option<(String, String)> {
    let db = crate::core::work::db_for(repo.journal.as_deref(), &repo.fael);
    let held = crate::core::work::held(&db, &repo.root.to_string_lossy());
    let (uid, title, _) = held.into_iter().next()?;
    Some((
        uid.clone(),
        format!(
            "fael: this worktree runs chunk {uid} \"{title}\" — its brief and hand-offs: fael chunk show {uid}\n"
        ),
    ))
}

/// PLAN-fael-visible-secretary chunk 4: the brief's line for the user — the
/// rows the agent was just handed, up to two named by key (else id) so each
/// is a `fael find` away. No row said = no line.
fn brief_line(c: &super::protocol::Ctx, said: &[&core::Row]) -> Option<String> {
    if !c.repo.cfg.notify_user || said.is_empty() {
        return None;
    }
    let ab = core::abbrev(&c.log);
    let named: Vec<String> = said
        .iter()
        .take(2)
        .map(|r| match &r.key {
            Some(k) => format!("#{k}"),
            None => ab.short(&r.id).to_string(),
        })
        .collect();
    let more = if said.len() > 2 { " …" } else { "" };
    Some(format!(
        "fael: briefed agent — {} {} ({}{more})",
        said.len(),
        if said.len() == 1 { "row" } else { "rows" },
        named.join(", ")
    ))
}

/// SPEC §11: the cheap checks — one line each, only when there is a problem.
/// Skipped while no log exists yet: warning about an empty missing log is
/// noise, and it saves a git spawn on every session start.
fn warnings(repo: &crate::Repo, adopted: bool) -> Vec<String> {
    let mut out = vec![];
    if !adopted {
        return out;
    }
    // doctor's [Wiring] note: a hook that shipped after the last install stays
    // off until then. Writes it now instead of asking the user to; only a pass
    // that fails (or leaves changes behind) falls back to the old warning.
    // a scratch repo under the temp dir never writes the real HOME's wiring (01M4NB34)
    let scratch = crate::core::work::in_tmp(&repo.root)
        && !crate::home().is_some_and(|h| crate::core::work::in_tmp(&h));
    if !scratch && crate::install::pending() > 0 {
        let wrote = crate::install::apply_pending();
        let behind = crate::install::pending();
        match wrote {
            Ok((n, trust)) if n > 0 && behind == 0 => out.push(format!(
                "fael: client wiring brought up to date ({n} change(s) written) — newer file \
                 pushes are on from the next session{}",
                if trust {
                    "; Codex first needs /hooks to trust the new hooks"
                } else {
                    ""
                }
            )),
            _ if behind > 0 => out.push(format!(
                "fael: {behind} client wiring change(s) behind this binary — some file pushes stay \
                 off; tell the user to run `fael upgrade`"
            )),
            _ => {}
        }
    }
    out
}

/// The branch this session started on, for the session Focus. Empty session
/// (no key for the Focus file), detached HEAD and the default branch
/// (`work_branch`) build no Focus.
fn start_branch(session: &str, root: &Path) -> Option<String> {
    if session.is_empty() {
        return None;
    }
    crate::journal::work_branch(root)
}
