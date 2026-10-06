//! The session-start event: kickoff rows for the repo, and the one-line
//! warning when `.fael/log` is gitignored by mistake.

use super::asks::{UsageMeta, hook_meta};
use super::focus;
use super::protocol::{Event, Reply, ctx};
use super::say::{Kind, Line, Outbox};
use super::state::{head_branch, prune_sessions, seen_path, session_key, state_dir};
use super::usage::record_usage;
use crate::{aliases, core, home};
use std::path::{Path, PathBuf};

pub(crate) fn session_start(e: &Event) -> Reply {
    let no = Reply::default;
    let c = match ctx(e) {
        Some(c) => c,
        None => return no(),
    };
    prune_sessions(&state_dir().join("sessions"));
    // detached: teammates' rows land before the first read, never awaited
    super::autosync::start(e);
    // a compacted context lost the rows pushed into it — tell them again
    if e.source.as_deref() == Some("compact") && !c.session.is_empty() {
        let _ = std::fs::remove_file(seen_path(&c.session, "", &c.repo.root));
    }
    let branch = start_branch(&c.session, &c.repo.root);
    // once per session: pick up renames committed since the last session, so
    // the read/edit push (which never spawns git) resolves them — and kickoff
    // keeps rows whose files were merely renamed
    let al = aliases::load(&c.repo, &c.log, true);
    // PLAN-fael-direction chunk 3: issues `to` this reader are the reader's
    // job, and urgent issues with no `to` belong to nobody — both list in
    // full above the count line; the count covers every open issue. `to` and
    // `urgent` never change push or find. Reader identity needs git, which
    // session-start already spawns (aliases refresh above, check-ignore
    // below); read/edit never compute it (SPEC fail examples).
    let reader = crate::writer(&c.repo);
    // one pass over the open rows: the Focus keys the branch rows carry, and
    // the to-do's issues — `find` hides closed and superseded either way
    let all: Vec<&core::Row> = core::find(&c.log, &core::Filter::default());
    let f = core::Focus::from_rows(branch.as_deref(), &all);
    focus::write(&c.session, &c.repo.root, &f);
    let work = core::on_work(&f, &all, &c.repo.cfg.anchor_prefixes);
    let open: Vec<&core::Row> = all.iter().copied().filter(|r| r.kind == "issue").collect();
    let t = todo(open, &reader, e.client.as_deref(), &work);
    let decisions: Vec<_> = if c.repo.cfg.session_decisions == 0 {
        vec![]
    } else {
        core::kickoff(
            &c.log,
            &core::Filter {
                kind: Some("decision".into()),
                ..core::Filter::default()
            },
            &c.repo.root,
            &al,
            &c.repo.cfg.anchor_prefixes,
        )
        .into_iter()
        .take(c.repo.cfg.session_decisions)
        .collect()
    };
    // chunk 5 wakes due revisits in CLI kickoff, but agents walk through
    // this door instead — due rows list here too, kickoff-ranked, minus ids
    // the to-do and decisions already show. Chunk 3 promises the to-do in
    // full under one shared budget, so it renders first: a flood of due
    // rows can cut the decisions, never the reader's own issues.
    let due: Vec<&core::Row> = {
        let (due, _) = core::with_due(&c.log, vec![], &c.repo.root, &al);
        let shown: std::collections::HashSet<&str> = t
            .listed
            .iter()
            .chain(decisions.iter())
            .map(|r| r.id.as_str())
            .collect();
        let due: Vec<&core::Row> = due
            .into_iter()
            .filter(|r| !shown.contains(r.id.as_str()))
            .collect();
        core::ranked(due, None, |_| 0, core::freshness(&c.repo.root, &al))
    };
    // one render, one budget: to-do first, then due, then decisions, a single cut line
    let shown: Vec<&core::Row> = t
        .listed
        .iter()
        .chain(due.iter())
        .chain(decisions.iter())
        .copied()
        .collect();
    let mut body = crate::find::branches::tag(
        core::render(&c.log, &shown, c.repo.cfg.kickoff_tokens),
        &c.tags,
    );
    if let Some(line) = count_line(&t) {
        body.push_str(&line);
    }
    if let Some(line) = gone_line(&c.repo, &c.log, &all, &reader) {
        body.push_str(&line);
    }
    let adopted = crate::journal::home(&c.repo).is_some();
    // like the read/edit push: usage counts only the ids render actually
    // said — rows the budget cut off never reached any context
    let n = body.lines().filter(|l| l.starts_with("- [")).count();
    let said = &shown[..n.min(shown.len())];
    let ids: Vec<String> = said.iter().map(|r| r.id.clone()).collect();
    // the rows, then each warning — the report rule rides the skill's
    // description (always loaded), the id rule that and the reject of an id
    // that does not exist (PLAN-fael-say-gate chunk 2)
    let mut out = Outbox::open(None);
    out.say(Line {
        kind: Kind::Brief,
        text: body,
    });
    for warn in warnings(&c.repo, adopted) {
        out.say(Line::notice(format!("{warn}\n")));
    }
    let mut r = out.reply();
    let Some(context) = r.context() else {
        return no();
    };
    // the session just began — no round completed yet, so no real tokens
    let meta = UsageMeta {
        said: r.said(),
        ..hook_meta(&c, None, false)
    };
    record_usage(
        &c.client,
        "session-start",
        &c.repo.root,
        context,
        &ids,
        &meta,
    );
    r.notice = brief_line(&c, said);
    r
}

/// PLAN-fael-visible-secretary chunk 4: the brief's line for the user — the
/// open issues the agent was just handed in full, up to two named by key
/// (else id) so each is a `fael find` away. No issue said = no line.
fn brief_line(c: &super::protocol::Ctx, said: &[&core::Row]) -> Option<String> {
    let issues: Vec<&core::Row> = said.iter().copied().filter(|r| r.kind == "issue").collect();
    if !c.repo.cfg.notify_user || issues.is_empty() {
        return None;
    }
    let ab = core::abbrev(&c.log);
    let named: Vec<String> = issues
        .iter()
        .take(2)
        .map(|r| match &r.key {
            Some(k) => format!("#{k}"),
            None => ab.short(&r.id).to_string(),
        })
        .collect();
    let more = if issues.len() > 2 { " …" } else { "" };
    Some(format!(
        "fael: briefed agent — {} open {} ({}{more})",
        issues.len(),
        if issues.len() == 1 { "issue" } else { "issues" },
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
    // `local` means the log is meant to stay out of git — an ignore is no problem
    if matches!(repo.cfg.store, core::Store::Tracked) && check_ignore_hit(&repo.root) {
        out.push(
            "fael: .fael/log is gitignored — rows stay on this machine, run fael doctor".into(),
        );
    }
    // doctor's [Wiring] note: a hook that shipped after the last install stays
    // off until then. Writes it now instead of asking the user to; only a pass
    // that fails (or leaves changes behind) falls back to the old warning.
    if crate::install::pending() > 0 {
        let wrote = crate::install::apply_pending();
        let behind = crate::install::pending();
        match wrote {
            Ok(n) if n > 0 && behind == 0 => out.push(format!(
                "fael: client wiring brought up to date ({n} change(s) written) — \
                 newer file pushes are on from the next session"
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
/// (no key for the Focus file) and detached HEAD (no branch) build no Focus.
fn start_branch(session: &str, root: &Path) -> Option<String> {
    if session.is_empty() {
        return None;
    }
    head_branch(root)
}

/// Open issues grouped for session start: mine (`to` = reader, the reader's
/// job), hot (urgent with no `to` — nobody owns them, so everyone sees them
/// in full) and work (tied to this branch, `core::on_work` — a count the
/// agent skims past hid the one issue on its plan). All list in full,
/// ranked; everything else counts only.
struct Todo<'a> {
    listed: Vec<&'a core::Row>,
    to_you: usize,
    to_you_urgent: usize,
    hot: usize,
    work: usize,
    total: usize,
}

/// `client` is the agent's own name (`opencode`, `codex`, `claude`): an issue
/// `--to opencode` reaches every OpenCode session in the repo, whoever runs it.
fn todo<'a>(
    open: Vec<&'a core::Row>,
    reader: &str,
    client: Option<&str>,
    work: &[&core::Row],
) -> Todo<'a> {
    let total = open.len();
    let mut mine = vec![];
    let mut unowned = vec![];
    let mut tied = vec![];
    let mut to_you_urgent = 0;
    for r in open {
        match r.to_who() {
            Some(t)
                if core::to_matches(t, reader)
                    || client.is_some_and(|c| t.eq_ignore_ascii_case(c)) =>
            {
                to_you_urgent += usize::from(r.urgent_value().is_some());
                mine.push(r);
            }
            None if r.urgent_value().is_some() => unowned.push(r),
            _ if work.iter().any(|w| w.id == r.id) => tied.push(r),
            _ => {}
        }
    }
    let (to_you, hot, work) = (mine.len(), unowned.len(), tied.len());
    let listed = core::ranked(
        mine.into_iter().chain(unowned).chain(tied).collect(),
        Some(reader),
        |_| 0,
        core::fresh_ts,
    );
    Todo {
        listed,
        to_you,
        to_you_urgent,
        hot,
        work,
        total,
    }
}

/// The one-line count summary, generated from the log, not prose. Zero open
/// issues = no line (chunk 1); zero segments are skipped, the total always
/// shows: `fael: 2 to you (1 urgent) · 1 urgent unassigned · 7 open — …`.
/// Kept short: it rides every session start, and the file-touch push of
/// each counted issue is the agent's to notice, not this line's to explain.
fn count_line(t: &Todo) -> Option<String> {
    if t.total == 0 {
        return None;
    }
    let mut parts = vec![];
    if t.to_you > 0 {
        parts.push(match t.to_you_urgent {
            0 => format!("{} to you", t.to_you),
            u => format!("{} to you ({u} urgent)", t.to_you),
        });
    }
    if t.hot > 0 {
        parts.push(format!("{} urgent unassigned", t.hot));
    }
    if t.work > 0 {
        parts.push(format!("{} tied to this branch", t.work));
    }
    parts.push(format!(
        "{} open {}",
        t.total,
        if t.total == 1 { "issue" } else { "issues" }
    ));
    Some(format!(
        "fael: {} — fael find --kind issue (MCP find kind=issue)\n",
        parts.join(" · ")
    ))
}

/// Open issues this reader holds whose branch is gone from the clone — the
/// work merged (or was dropped) and nobody closed the issue with its answer.
/// Says what is true, never "merged": a deleted branch is all fael sees. A
/// file read per hold, no git spawn; no journal = no claims = no line.
// ponytail: a squash-merged branch kept locally still counts as alive, and a
// hold made under the same name on another machine reads as gone here —
// add an ancestry check (a git spawn) only if the miss shows up in use.
fn gone_line(
    repo: &crate::Repo,
    log: &core::Log,
    open: &[&core::Row],
    reader: &str,
) -> Option<String> {
    let common = repo.journal.as_deref().and_then(Path::parent)?;
    let gone: Vec<(&core::Row, &str)> = open
        .iter()
        .filter(|r| r.kind == "issue" && r.by == reader)
        .filter_map(|r| r.held().map(|h| (*r, h)))
        .filter(|(_, h)| !crate::journal::branch_alive(common, h))
        .collect();
    let (first, branch) = gone.first()?;
    let ab = core::abbrev(log);
    let id = ab.short(&first.id);
    let more = match gone.len() {
        1 => String::new(),
        n => format!(" +{} more", n - 1),
    };
    Some(format!(
        "fael: {id} held @{branch}{more} — branch gone, issue still open: \
         fael close {id} \"<what shipped, where>\" (MCP close), or fael claim {id} --force\n"
    ))
}

/// `git check-ignore` is ~8 of session-start's ~10 ms, so its answer is cached
/// per worktree, keyed by the mtime+size of every file that can change it.
// ponytail: stamps root and .fael .gitignore, info/exclude, the default global
// ignore and ~/.gitconfig (a moved core.excludesFile) — edits inside a custom
// excludesFile or a worktree's common info/exclude are missed until another
// stamp moves; `fael doctor` always asks git.
fn check_ignore_hit(root: &Path) -> bool {
    let home = home().unwrap_or_default();
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".config"));
    let stamp: String = [
        root.join(".gitignore"),
        root.join(".fael/.gitignore"),
        root.join(".fael/log/.gitignore"),
        root.join(".git/info/exclude"),
        xdg.join("git/ignore"),
        home.join(".gitconfig"),
    ]
    .iter()
    .map(|p| match std::fs::metadata(p) {
        Ok(m) => {
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            format!("{}.{},", t.map_or(0, |t| t.as_nanos()), m.len())
        }
        Err(_) => "-,".into(),
    })
    .collect();
    let cache = state_dir()
        .join("ignore")
        .join(session_key(&root.to_string_lossy()));
    if let Some(hit) = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|s| s.strip_prefix(&stamp).map(|v| v == "1"))
    {
        return hit;
    }
    let hit = ignore_source(root).is_some_and(|s| !deliberate(&s));
    let _ = std::fs::create_dir_all(cache.parent().unwrap_or(root));
    let _ = std::fs::write(&cache, format!("{stamp}{}", u8::from(hit)));
    hit
}

/// Which ignore rule keeps `.fael/log` out of git (`git check-ignore -v`'s
/// `<source>:<line>:<pattern>\t<path>`), or `None` when it is tracked.
pub(crate) fn ignore_source(root: &Path) -> Option<String> {
    crate::git(root, &["check-ignore", "-v", ".fael/log"])
}

/// `.git/info/exclude` is local to this clone and never shared, so ignoring the log
/// there is a choice (a public repo keeping its memory private), not a mistake.
pub(crate) fn deliberate(source: &str) -> bool {
    source.replace('\\', "/").contains("info/exclude:")
}
