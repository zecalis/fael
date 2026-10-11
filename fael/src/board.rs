//! `fael board [--json] [--open]` (PLAN-fael-board b3b, SPEC §10): every registered repo's
//! plans and chunks, and four lists of uids across them — needs you (§5), the queue (§3),
//! running, blocked. The app's only read; `"v": 1` is the contract
//! (`tests/golden/board-v1.json`). Live git: one worktree list and one base per project.
//! A project fael cannot read (a newer plans.db) stays on with its `error` and empty lists,
//! and the command exits 1: never an empty board that reads as live (issue 01M4N4CV7).

use crate::plan::db;
use crate::{Args, Repo, git, repo, repo_at};
use fael_core::plan::{BoardChunk, BoardPlan, Store};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Serialize)]
struct Board {
    v: u32,
    at: String,
    needs_you: Vec<Need>,
    queue: Vec<String>,
    running: Vec<String>,
    blocked: Vec<String>,
    projects: Vec<Project>,
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
    plans: Vec<BoardPlan>,
    chunks: Vec<BoardChunk>,
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
    a.only("board", &["json", "open", "plan"])?;
    if let Ok(r) = repo()
        && db(&r).exists()
    {
        register(&r);
    }
    let (at, stale) = clock();
    let mut projects = Vec::new();
    let mut broken = false;
    for root in registered() {
        match project(&root, &at[..10], &stale, a.has("open")) {
            Ok(Some(p)) => projects.push(p),
            Ok(None) => {}
            // one broken repo never hides the others, and is never silent
            Err(e) => {
                eprintln!("fael board: {}: {e}", root.display());
                broken = true;
                projects.push(Project {
                    error: Some(e),
                    ..Project::named(&root)
                });
            }
        }
    }
    // an agent's cut: one plan's chunks, the projects that have it (and the unreadable)
    if let Some(n) = a.one("plan") {
        for p in &mut projects {
            p.plans.retain(|x| x.name == n);
            p.chunks.retain(|c| c.plan == n);
        }
        projects.retain(|p| !p.plans.is_empty() || p.error.is_some());
    }
    let b = lists(at, projects);
    if a.has("json") {
        let s = serde_json::to_string_pretty(&b).map_err(|e| e.to_string())?;
        println!("{s}");
    } else {
        print!("{}", text(&b));
    }
    Ok(if broken {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
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
            plans: Vec::new(),
            chunks: Vec::new(),
            error: None,
        }
    }
}

fn project(root: &Path, today: &str, stale: &str, open: bool) -> Result<Option<Project>, String> {
    let Ok(r) = repo_at(root) else {
        return Ok(None);
    };
    let path = db(&r);
    if !path.exists() {
        return Ok(None);
    }
    let mut b = Store::open(&path)?.board(today, stale)?;
    // SPEC §4: the planner's one read — every open chunk, with its scope and overlaps
    if open {
        b.chunks.retain(|c| c.state == "open");
    }
    let live: HashMap<&str, &str> = b
        .chunks
        .iter()
        .filter_map(|c| c.run.as_ref())
        .filter(|r| r.ended.is_none())
        .filter_map(|r| Some((r.worktree.as_deref()?, r.id.as_str())))
        .collect();
    let worktrees = worktrees(&r.root, r.cfg.plan_worktrees.as_deref(), &live);
    Ok(Some(Project {
        base: git(&r.root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]),
        worktrees,
        plans: b.plans,
        chunks: b.chunks,
        ..Project::named(root)
    }))
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

/// The four lists, in display order across every project.
fn lists(at: String, projects: Vec<Project>) -> Board {
    let all: Vec<&BoardChunk> = projects.iter().flat_map(|p| &p.chunks).collect();
    let uids =
        |f: Pick| -> Vec<String> { all.iter().filter(|c| f(c)).map(|c| c.uid.clone()).collect() };
    // SPEC §5 order: the owner's questions, then review, ended runs, inbox drafts
    let kinds: [(&'static str, Pick); 4] = [
        // an md plan's `(wait …)` names no `on`: the owner is the one who can move it
        ("waiting", &|c| {
            c.state == "waiting"
                && c.wait
                    .as_ref()
                    .is_some_and(|w| matches!(w.on.as_deref(), Some("owner") | None))
        }),
        ("review", &|c| c.state == "review"),
        ("ended", &|c| c.state == "running" && c.ended),
        ("inbox", &|c| c.state == "draft" && c.plan == "inbox"),
    ];
    let needs_you = kinds
        .iter()
        .flat_map(|(kind, f)| uids(f).into_iter().map(|uid| Need { kind, uid }))
        .collect();
    // SPEC §3: pin, due soonest, unblocks most, plan rank — stable, so plan order breaks ties
    let mut queue: Vec<&&BoardChunk> = all.iter().filter(|c| c.ready).collect();
    queue.sort_by_key(|c| {
        (
            c.pin.is_none(),
            c.pin,
            c.due.is_none(),
            c.due.clone(),
            Reverse(c.unblocks),
            c.rank.is_none(),
            c.rank,
        )
    });
    let queue = queue.iter().map(|c| c.uid.clone()).collect();
    let running = uids(&|c| c.state == "running" && !c.ended);
    let blocked = uids(&|c| c.state == "open" && !c.blocked_by.is_empty());
    Board {
        v: 1,
        at,
        needs_you,
        queue,
        running,
        blocked,
        projects,
    }
}

/// The same lists for a person at a terminal: one line per chunk.
fn text(b: &Board) -> String {
    let all: HashMap<&str, &BoardChunk> = b
        .projects
        .iter()
        .flat_map(|p| &p.chunks)
        .map(|c| (c.uid.as_str(), c))
        .collect();
    let line = |uid: &str, tag: &str| {
        let c = all[uid];
        let label = c
            .label
            .as_deref()
            .map(|l| format!(" {l}"))
            .unwrap_or_default();
        let extra = match tag {
            "waiting" => c.wait.as_ref().and_then(|w| w.text.clone()),
            "blocked" => Some(
                c.blocked_by
                    .iter()
                    .map(|x| x.what.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .map(|w| format!("after {w}")),
            _ => None,
        }
        .map(|x| format!(" — {x}"))
        .unwrap_or_default();
        // ponytail: a terminal line, cut at 80 chars; the app reads --json
        let title: String = match c.title.char_indices().nth(80) {
            Some((i, _)) => format!("{}…", &c.title[..i]),
            None => c.title.clone(),
        };
        format!("  {tag:<8} {uid}  {}{label}  {title}{extra}\n", c.plan)
    };
    let mut out = String::new();
    for p in b.projects.iter().filter(|p| p.error.is_some()) {
        out.push_str(&format!(
            "unreadable {}: {}\n",
            p.root,
            p.error.as_deref().unwrap_or("")
        ));
    }
    for (head, rows) in [
        (
            "needs you",
            b.needs_you
                .iter()
                .map(|n| (n.uid.as_str(), n.kind))
                .collect::<Vec<_>>(),
        ),
        (
            "queue",
            b.queue.iter().map(|u| (u.as_str(), "ready")).collect(),
        ),
        (
            "running",
            b.running.iter().map(|u| (u.as_str(), "running")).collect(),
        ),
        (
            "blocked",
            b.blocked.iter().map(|u| (u.as_str(), "blocked")).collect(),
        ),
    ] {
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("{head}\n"));
        for (uid, tag) in rows {
            out.push_str(&line(uid, tag));
        }
    }
    if out.is_empty() {
        out.push_str("nothing on the board — `fael chunk add` or `fael plan import` in a repo\n");
    }
    out
}

/// `~/.local/state/fael/projects`: one main-checkout root per line — every repo the board
/// covers (SPEC §10). The app runs `fael board` from home, outside any repo.
fn registry() -> PathBuf {
    fael_core::stats::state_dir().join("projects")
}

fn registered() -> Vec<PathBuf> {
    let text = std::fs::read_to_string(registry()).unwrap_or_default();
    let all: Vec<PathBuf> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    let keep: Vec<PathBuf> = all.iter().filter(|p| !scratch(p)).cloned().collect();
    // prune what older faels registered (issue 01M4MH3H7); a failed write only skips again
    if keep.len() < all.len() {
        let text: String = keep.iter().map(|p| format!("{}\n", p.display())).collect();
        let _ = std::fs::write(registry(), text);
    }
    keep
}

/// A repo under the temp dir (an agent's or a test's scratch repo) belongs only on a
/// registry that is itself under the temp dir — a test's `FAEL_STATE_DIR`.
fn scratch(root: &Path) -> bool {
    // canonical (`/private/var/…` on macOS) through the nearest dir that exists yet
    let tmp = |p: &Path| {
        let p = p
            .ancestors()
            .find_map(|a| Some(a.canonicalize().ok()?.join(p.strip_prefix(a).ok()?)))
            .unwrap_or_else(|| p.to_path_buf());
        [std::env::temp_dir(), PathBuf::from("/tmp")]
            .iter()
            .filter_map(|d| d.canonicalize().ok())
            .any(|d| p.starts_with(d))
    };
    tmp(root) && !tmp(&fael_core::stats::state_dir())
}

/// Put this repo on the board: called by every command that writes its plans.db.
pub(crate) fn register(r: &Repo) {
    // the main checkout, not this worktree: `<main>/.git/fael` is the journal
    let root = r
        .journal
        .as_deref()
        .and_then(Path::parent)
        .filter(|g| g.file_name().is_some_and(|n| n == ".git"))
        .and_then(Path::parent)
        .unwrap_or(&r.root);
    if scratch(root) || registered().iter().any(|p| p == root) {
        return;
    }
    let f = registry();
    let line = format!("{}\n", root.display());
    let ok = f
        .parent()
        .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
        && std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&f)
            .and_then(|mut h| std::io::Write::write_all(&mut h, line.as_bytes()))
            .is_ok();
    if !ok {
        eprintln!("fael: could not add {} to {}", root.display(), f.display());
    }
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
