//! `fael board [--json] [--open] [--watch]` (PLAN-fael-board b3b, SPEC §10): every registered repo's
//! plans and chunks, and four lists of uids across them — needs you (§5), the queue (§3),
//! running, blocked. The app's only read; `"v": 1` is the contract
//! (`tests/golden/board-v1.json`). Live git: one worktree list and one base per project.
//! A project fael cannot read (a newer crew.db) stays on with its `error` and empty lists,
//! and the command exits 1: never an empty board that reads as live (issue 01M4N4CV7).

use crate::plan::db;
use crate::{Args, git, repo, repo_at};
use fael_crew::{BoardChunk, BoardPlan, Models, Store};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

#[derive(Serialize)]
struct Board<'a> {
    v: u32,
    at: String,
    needs_you: Vec<Need>,
    queue: Vec<String>,
    running: Vec<String>,
    blocked: Vec<String>,
    /// o4: the queue split by overlap class (additive, `v` stays 1 — the app
    /// keeps reading `queue`; a rename would need v2 per 01M581CX).
    /// `queue_exclusive` = ready with no clash vs a live run: solo-startable.
    queue_exclusive: Vec<String>,
    /// `queue_joint` = ready with a prefix-only clash vs a live run: startable
    /// beside it — `with` names the holder, `as` the ready joint mode (lane).
    queue_joint: Vec<JointSpot>,
    /// `blocked_on_overlap` = ready with an exact-file clash vs a live run:
    /// a solo (or lane) start still refuses — `with` names the holder, `as`
    /// the joint mode that unblocks it (pair; follow also works).
    blocked_on_overlap: Vec<JointSpot>,
    /// cost-per-feature read (run-telemetry t5): merged over every project
    models: Models,
    projects: Vec<&'a Project>,
}

/// One ready chunk's joint slot: what `chunk start` would take beside the holder.
#[derive(Serialize)]
struct JointSpot {
    uid: String,
    with: String,
    #[serde(rename = "as")]
    mode: String,
}

#[derive(Serialize)]
struct Need {
    kind: &'static str,
    uid: String,
}

#[derive(Serialize)]
struct Project {
    name: String,
    root: String,
    base: Option<String>,
    worktrees: Vec<Worktree>,
    /// every local branch but the base's, by push state; absent = none
    #[serde(skip_serializing_if = "Vec::is_empty")]
    branches: Vec<branches::Branch>,
    /// `.fael/shared`: paths nobody's scope claims; absent = none
    #[serde(skip_serializing_if = "Vec::is_empty")]
    shared: Vec<String>,
    plans: Vec<BoardPlan>,
    chunks: Vec<BoardChunk>,
    /// cost-per-feature read (run-telemetry t5): this repo's runs; the outer
    /// board merges every project into one Models card
    models: Models,
    /// absent in an agent's cut (`--open`, `--plan`): the app reads the whole board
    #[serde(skip_serializing_if = "Option::is_none")]
    ledger: Option<ledger::Ledger>,
    /// why this project could not be read; only then present
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// A pool slot (SPEC §8): `run` = the live start R on it, null = free.
#[derive(Serialize)]
struct Worktree {
    path: String,
    branch: Option<String>,
    run: Option<String>,
}

pub(crate) fn cmd(a: &Args) -> Result<ExitCode, String> {
    a.only("board", &["json", "open", "plan", "drop", "watch"])?;
    if let Some(root) = a.one("drop") {
        return registry::unregister(&root);
    }
    if let Ok(r) = repo()
        && db(&r).exists()
    {
        register(&r);
    }
    if a.has("watch") {
        if !a.has("json") {
            return Err("rejected: --watch prints board lines: `fael board --json --watch`".into());
        }
        return watch::run(a);
    }
    let (at, stale) = clock();
    // one thread per repo: each read waits mostly on its own git/gh children
    let roots = registered();
    let mut projects: Vec<Project> = std::thread::scope(|sc| {
        let jobs: Vec<_> = roots
            .iter()
            .map(|root| sc.spawn(|| read(root, &at, &stale, a)))
            .collect();
        jobs.into_iter()
            .filter_map(|j| j.join().expect("board read panicked"))
            .collect()
    });
    prs::live_branches(&mut projects);
    let broken = projects.iter().any(|p| p.error.is_some());
    let b = lists(at, projects.iter().collect());
    if a.has("json") {
        // compact: 0.87 MB where pretty was 1.13 (5 projects); the app parses, nobody reads it
        let s = serde_json::to_string(&b).map_err(|e| e.to_string())?;
        println!("{s}");
    } else {
        let mut out = text::text(&b, a.one("plan").is_some());
        // empty registry, not empty projects: a --plan/--open cut can hide
        // every project while the registry still names repos (01M5H0VZ)
        if registry::registered().is_empty() {
            let cwd = std::env::current_dir()
                .and_then(|d| d.canonicalize())
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            out.push_str(&format!(
                "repos: (none) — cwd {}, registry {}\n",
                cwd.display(),
                registry::registry().display()
            ));
        }
        print!("{out}");
    }
    Ok(ExitCode::from(u8::from(broken)))
}

/// One registered repo as the board shows it, `--open` and `--plan` applied; None = not on it.
fn read(root: &Path, at: &str, stale: &str, a: &Args) -> Option<Project> {
    let cut = a.has("open") || a.one("plan").is_some();
    let mut p = match project(root, &at[..10], stale, a.has("open"), cut) {
        Ok(p) => p?,
        // one broken repo never hides the others, and is never silent
        Err(e) => {
            eprintln!("fael board: {}: {e}", root.display());
            Project {
                error: Some(e),
                ..Project::named(root)
            }
        }
    };
    // an agent's cut: one plan's chunks, the projects that have it (and the unreadable);
    // `name` or `app/name`, the key `plan new` and `plan export` print
    if let Some(n) = a.one("plan") {
        let is = |app: &str, name: &str| name == n || format!("{app}/{name}") == n;
        p.plans.retain(|x| is(&x.app, &x.name));
        p.chunks.retain(|c| is(&c.app, &c.plan));
        if p.plans.is_empty() && p.error.is_none() {
            return None;
        }
    }
    Some(p)
}

/// Now, and the RFC 3339 time 30 min ago: a live run last seen before it is `stalled`.
pub(crate) fn clock() -> (String, String) {
    let ms = fael_core::now_ms();
    let stale = fael_core::rfc3339(ms.saturating_sub(30 * 60 * 1000));
    (fael_core::rfc3339(ms), stale)
}

impl Project {
    fn named(root: &Path) -> Project {
        Project {
            name: root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            root: root.to_string_lossy().into_owned(),
            base: None,
            worktrees: Vec::new(),
            branches: Vec::new(),
            shared: Vec::new(),
            plans: Vec::new(),
            chunks: Vec::new(),
            models: Models::default(),
            ledger: None,
            error: None,
        }
    }
}

/// `owner/name` of a checkout's origin: `git@github.com:o/n.git`, `https://…/o/n`.
pub(crate) fn remote(root: &Path) -> Option<String> {
    let url = git(root, &["remote", "get-url", "origin"])?;
    let mut parts = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit(['/', ':']);
    let name = parts.next()?;
    Some(format!("{}/{name}", parts.next()?))
}

fn project(
    root: &Path,
    today: &str,
    stale: &str,
    open: bool,
    cut: bool,
) -> Result<Option<Project>, String> {
    let Ok(r) = repo_at(root) else {
        return Ok(None);
    };
    let path = db(&r);
    if !path.exists() {
        return Ok(None);
    }
    let s = Store::open(&path)?;
    let mut b = s.board(today, stale)?;
    let base = git(&r.root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]);
    fill_left_stale(&s, &r, &mut b.chunks);
    let shared = prs::open_prs(&mut b.chunks, &r.root, base.as_deref());
    let branches = branches::branches(&r.root, base.as_deref(), &b.chunks);
    // before `--open` drops the running chunks: their live runs hold worktrees (a lead picks
    // a free one from this read)
    let live: HashMap<&str, &str> = b
        .chunks
        .iter()
        .filter_map(|c| c.run.as_ref())
        .filter(|r| r.ended.is_none())
        .filter_map(|r| Some((r.worktree.as_deref()?, r.id.as_str())))
        .collect();
    let worktrees = worktrees(&r.root, r.cfg.plan_worktrees.as_deref(), &live);
    // SPEC §4: the planner's one read — every open chunk, with its scope and overlaps
    if open {
        b.chunks.retain(|c| c.state == "open");
    }
    Ok(Some(Project {
        base,
        worktrees,
        branches,
        shared,
        plans: b.plans,
        chunks: b.chunks,
        models: b.models,
        ledger: if cut {
            None
        } else {
            Some(ledger::read(&r, &s.fixers()?))
        },
        ..Project::named(root)
    }))
}

/// `left` from the latest hand-off, `left_open` the still-open subset, `stale`
/// from the brief against the ledger (read once).
fn fill_left_stale(s: &Store, r: &crate::Repo, chunks: &mut [BoardChunk]) {
    let log = crate::read(r);
    let shut = fael_core::closed(&log);
    let gone = fael_core::superseded(&log);
    let keys = fael_core::keys(&log, None);
    let states = s.chunk_states().unwrap_or_default();
    let open_tok = |tok: &str| -> bool {
        let up = tok.to_uppercase();
        let mut seen_chunk = false;
        let mut chunk_open = false;
        for (uid, st) in &states {
            if uid.starts_with(&up) || up.starts_with(uid.as_str()) {
                seen_chunk = true;
                if !matches!(st.as_str(), "done" | "dropped" | "replaced") {
                    chunk_open = true;
                }
            }
        }
        if seen_chunk {
            return chunk_open;
        }
        match fael_core::ref_state(&log, tok) {
            fael_core::Ref::One(row) => {
                !shut.contains(row.id.as_str()) && !gone.contains(row.id.as_str())
            }
            // ambiguous: surface it — the owner decides
            fael_core::Ref::Many(_) => true,
            fael_core::Ref::Missing => false,
        }
    };
    for c in chunks {
        if let Some(h) = &c.handoff {
            c.left = crate::chunk_left::listed(&h.text);
            c.left_open = c.left.iter().filter(|t| open_tok(t)).cloned().collect();
        }
        if matches!(c.state.as_str(), "done" | "dropped" | "replaced") {
            continue;
        }
        if let Ok((title, brief, issues)) = s.stale_input(&c.uid) {
            c.stale = crate::chunk_scope::stale_warnings_in(
                &title,
                &brief,
                &issues,
                &log,
                &c.uid,
                (&shut, &keys),
            );
        }
    }
}

/// Every linked worktree; a repo with none pools its main checkout as one slot.
/// `[plan] worktrees = "<prefix>"` keeps only the linked ones whose path or dir name starts
/// with it (`wt-fael-agent-` leaves topic worktrees out of the pool).
fn worktrees(root: &Path, prefix: Option<&str>, live: &HashMap<&str, &str>) -> Vec<Worktree> {
    let list = git(root, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    let mut all: Vec<Worktree> = list
        .split("\n\n")
        .filter(|b| !b.lines().any(|l| l == "bare"))
        .filter_map(|b| {
            let path = b.lines().find_map(|l| l.strip_prefix("worktree "))?;
            // a run names its worktree canonicalized (`/private/var/…` on macOS)
            let path = Path::new(path)
                .canonicalize()
                .map_or_else(|_| path.to_string(), |p| p.to_string_lossy().into_owned());
            Some(Worktree {
                run: live.get(path.as_str()).map(|r| r.to_string()),
                branch: b
                    .lines()
                    .find_map(|l| l.strip_prefix("branch refs/heads/"))
                    .map(String::from),
                path,
            })
        })
        .collect();
    if all.len() > 1 {
        all.remove(0);
        if let Some(p) = prefix {
            all.retain(|w| {
                w.path.starts_with(p)
                    || Path::new(&w.path)
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(p))
            });
        }
    }
    all
}

type Pick<'a> = &'a dyn Fn(&BoardChunk) -> bool;

/// A running chunk whose branch already merged: the owner landed it while the
/// agent still runs — stop or rebase. Git answers it offline via
/// `project.branches[]` (merged = content on the base); fires on merge, never
/// closes the chunk. Per project: a branch name can repeat repos.
fn merged_running(projects: &[&Project]) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for p in projects {
        let m: std::collections::HashSet<&str> = p
            .branches
            .iter()
            .filter(|b| b.state == "merged")
            .map(|b| b.name.as_str())
            .collect();
        for c in &p.chunks {
            let on_merged = c.state == "running"
                && c.run
                    .as_ref()
                    .and_then(|r| r.branch.as_deref())
                    .is_some_and(|b| m.contains(b));
            if on_merged {
                out.insert(c.uid.clone());
            }
        }
    }
    out
}

/// The four lists, in display order across every project.
fn lists(at: String, projects: Vec<&Project>) -> Board<'_> {
    let all: Vec<&BoardChunk> = projects.iter().flat_map(|p| &p.chunks).collect();
    let uids =
        |f: Pick| -> Vec<String> { all.iter().filter(|c| f(c)).map(|c| c.uid.clone()).collect() };
    // done chunks whose linked issue is still open (l2): fael never closes it
    let fixed: Vec<String> = projects
        .iter()
        .flat_map(|p| p.ledger.iter().flat_map(|l| l.open_done()))
        .collect();
    let merged = merged_running(&projects);
    // SPEC §5 order: the owner's questions, then review, ended runs, inbox drafts
    let kinds: [(&'static str, Pick); 6] = [
        // an md plan's `(wait …)` names no `on`: the owner is the one who can move it
        ("waiting", &|c| {
            c.state == "waiting"
                && c.wait
                    .as_ref()
                    .is_some_and(|w| matches!(w.on.as_deref(), Some("owner") | None))
        }),
        ("review", &|c| c.state == "review"),
        ("ended", &|c| c.state == "running" && c.ended),
        ("merged", &|c| merged.contains(&c.uid)),
        ("inbox", &|c| c.state == "draft" && c.plan == "inbox"),
        ("issue-open", &|c| fixed.contains(&c.uid)),
    ];
    let needs_you = kinds
        .iter()
        .flat_map(|(kind, f)| uids(f).into_iter().map(|uid| Need { kind, uid }))
        .collect();
    // unblocks most first — stable, so plan order breaks ties
    let mut ordered: Vec<&&BoardChunk> = all.iter().filter(|c| c.ready).collect();
    ordered.sort_by_key(|c| Reverse(c.unblocks));
    let queue: Vec<String> = ordered.iter().map(|c| c.uid.clone()).collect();
    let running = uids(&|c| c.state == "running" && !c.ended);
    let blocked = uids(&|c| c.state == "open" && !c.blocked_by.is_empty());
    // o4: split the queue by clash vs a live run (the start gate's rule, via
    // o1's per-pair classes): exclusive = no running holder in its classes,
    // joint = prefix-only (lane), blocked_on_overlap = exact (pair). Each
    // bucket keeps queue order; `queue` itself stays for the app (v stays 1).
    // A `--open`/`--plan` cut drops the running chunks, but an open chunk's
    // classes still name the cut-away holder — so a partner absent from this
    // cut counts as a holder too (it can only be running or review).
    let live: std::collections::HashSet<&str> = running.iter().map(String::as_str).collect();
    let by_uid: HashMap<&str, &BoardChunk> = all.iter().map(|c| (c.uid.as_str(), *c)).collect();
    let mut queue_exclusive = Vec::new();
    let mut queue_joint = Vec::new();
    let mut blocked_on_overlap = Vec::new();
    for uid in &queue {
        let Some(c) = by_uid.get(uid.as_str()) else {
            continue;
        };
        let mut exact: Option<&str> = None;
        let mut prefix: Option<&str> = None;
        for (partner, class) in &c.overlap_classes {
            let holder = live.contains(partner.as_str()) || !by_uid.contains_key(partner.as_str());
            if !holder {
                continue;
            }
            if *class == fael_core::OverlapClass::Exact {
                if exact.is_none() {
                    exact = Some(partner);
                }
            } else if prefix.is_none() {
                prefix = Some(partner);
            }
        }
        if let Some(holder) = exact {
            blocked_on_overlap.push(JointSpot {
                uid: uid.clone(),
                with: holder.to_string(),
                mode: "pair".to_string(),
            });
        } else if let Some(holder) = prefix {
            queue_joint.push(JointSpot {
                uid: uid.clone(),
                with: holder.to_string(),
                mode: "lane".to_string(),
            });
        } else {
            queue_exclusive.push(uid.clone());
        }
    }
    Board {
        v: 1,
        at,
        needs_you,
        queue,
        running,
        blocked,
        queue_exclusive,
        queue_joint,
        blocked_on_overlap,
        models: Models::merge(
            &projects
                .iter()
                .map(|p| p.models.clone())
                .collect::<Vec<_>>(),
        ),
        projects,
    }
}

#[path = "board_branches.rs"]
mod branches;
#[path = "board_ledger.rs"]
mod ledger;
#[path = "board_git.rs"]
pub(crate) mod prs;
#[path = "board_registry.rs"]
mod registry;
#[path = "board_text.rs"]
mod text;
#[path = "board_watch.rs"]
mod watch;

pub(crate) use registry::{register, registered};
pub(crate) use watch::git_output;

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
