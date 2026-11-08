//! What the default branch did since a running chunk's base, so its brief can be judged
//! without a hand-run `git log`: commits that touch its files (not its own), and chunks and
//! ledger rows closed since it started that bear on it. `chunk status` prints it, the board
//! says `drift N`. Facts only — whether the brief is out of date is the agent's call.
//! One `git log base..main --name-only` per distinct base; no per-file git calls.

use super::board::prs::names_pr;
use crate::{Repo, git};
use fael_crew::{BoardChunk, Live, Shut, Store};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// One commit of `base..main` with the paths it touched.
pub(crate) struct Commit {
    sha: String,
    subject: String,
    files: Vec<String>,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub(crate) struct Moved {
    /// commits the default branch is past the chunk's base
    pub behind: usize,
    /// `<sha> <subject>` of those touching the chunk's files, minus its own branch and PR
    pub commits: Vec<String>,
    /// chunks and ledger rows closed since the start that overlap its files, and the closed
    /// rows its brief cites (the board's `stale`)
    pub closed: Vec<String>,
    /// how long ago `origin` was last fetched; a drift of 0 is only as fresh as this
    #[serde(skip)]
    pub fetched: Option<String>,
}

impl Moved {
    pub(crate) fn n(&self) -> usize {
        self.commits.len() + self.closed.len()
    }

    fn age(&self) -> String {
        self.fetched
            .as_ref()
            .map(|f| format!(" (origin fetched {f})"))
            .unwrap_or_default()
    }

    /// One line for `chunk done` (read it before done ends the run: drift needs a live run);
    /// None = nothing moved (silent).
    pub(crate) fn line(&self) -> Option<String> {
        (self.n() > 0).then(|| {
            format!(
                "drift: {} commit(s) on main touch this chunk's files, {} closed row(s) bear on it{} — `fael chunk status {{uid}}` lists them\n",
                self.commits.len(),
                self.closed.len(),
                self.age()
            ) + &self.next()
        })
    }

    /// The commands to act on it — only when a commit touches the files; facts, not a verdict.
    fn next(&self) -> String {
        match self.commits.is_empty() {
            true => String::new(),
            false => "  to take it in: `git rebase origin/main`; to tell the next agent: `fael chunk note {uid} \"<text>\"`\n".into(),
        }
    }

    /// The `drift` block of `chunk status`; the lists only when there is something to list.
    pub(crate) fn text(&self, base: &str, uid: &str) -> String {
        let mut o = format!(
            "  drift: main is {} commit(s) past base {base}; {} touch the files, {} closed row(s) bear on it{}\n",
            self.behind,
            self.commits.len(),
            self.closed.len(),
            self.age()
        );
        for c in &self.commits {
            o += &format!("    {c}\n");
        }
        for c in &self.closed {
            o += &format!("    closed: {c}\n");
        }
        o += &self.next().replace("{uid}", uid);
        o += &self.next();
        o
    }
}

/// `git log <base>..<main> --name-only`, newest first; None = git could not say.
fn main_log(root: &Path, main: &str, base: &str) -> Option<Vec<Commit>> {
    let range = format!("{base}..{main}");
    let args = [
        "-c",
        "core.quotepath=off",
        "log",
        "--name-only",
        "--format=%x01%H%x09%s",
        &range,
    ];
    let (code, out) = super::board::git_output(root, &args)?;
    (code == Some(0)).then_some(())?;
    Some(
        out.split('\x01')
            .filter_map(|rec| {
                let mut l = rec.lines();
                let (sha, subject) = l.next()?.split_once('\t')?;
                Some(Commit {
                    sha: sha.into(),
                    subject: subject.into(),
                    files: l.filter(|f| !f.is_empty()).map(String::from).collect(),
                })
            })
            .collect(),
    )
}

/// The drift of one live run. `stale` = the brief's closed-row warnings (`stale_warnings`).
fn moved(
    log: &[Commit],
    live: &Live,
    ledger: &fael_core::Log,
    shut: &[Shut],
    stale: &[String],
) -> Moved {
    // the commits its own worktree already holds (a branch it merged main into, a rebase)
    let own: HashSet<String> = live
        .worktree
        .as_deref()
        .zip(live.base.as_deref())
        .and_then(|(wt, base)| git(Path::new(wt), &["rev-list", &format!("{base}..HEAD")]))
        .map(|o| o.lines().map(String::from).collect())
        .unwrap_or_default();
    let commits = log
        .iter()
        .filter(|c| !own.contains(&c.sha))
        .filter(|c| !live.pr.is_some_and(|n| names_pr(&c.subject, n)))
        .filter(|c| fael_core::overlap(&c.files, &live.files))
        .map(|c| format!("{} {}", &c.sha[..c.sha.len().min(9)], c.subject))
        .collect();
    let mut closed: Vec<String> = stale
        .iter()
        .map(|w| w.strip_prefix("warning: ").unwrap_or(w).to_string())
        .collect();
    let mut seen = HashSet::new();
    let started = fael_core::ts_ms(&live.started);
    for c in ledger
        .closes
        .iter()
        .filter(|c| fael_core::ts_ms(&c.ts) > started)
        // the run's own `fael close` is its work, not drift
        .filter(|c| live.session.is_none() || c.session() != live.session.as_deref())
    {
        let Some(id) = c.reference.as_deref() else {
            continue;
        };
        let row = ledger.rows.iter().find(|r| r.id == id);
        // a cited row is in `stale` already (by id prefix or key)
        let said = |r: &fael_core::Row| {
            stale.iter().any(|w| {
                w.contains(&id[..id.len().min(8)])
                    || r.key.as_deref().is_some_and(|k| w.contains(k))
            })
        };
        if row.is_some_and(|r| fael_core::overlap(&r.files, &live.files) && !said(r))
            && seen.insert(id)
        {
            let first = c.text.lines().next().unwrap_or("");
            closed.push(format!("{id} — {first}"));
        }
    }
    for s in shut
        .iter()
        .filter(|s| s.at > live.started && s.uid != live.uid)
        .filter(|s| fael_core::overlap(&s.scope, &live.files))
    {
        let label = s
            .label
            .as_deref()
            .map(|l| format!(" {l}"))
            .unwrap_or_default();
        closed.push(format!("chunk {}{label} — {}", s.uid, s.title));
    }
    Moved {
        behind: log.len(),
        commits,
        closed,
        fetched: None,
    }
}

/// `chunk status`: one running chunk's drift; None = not running, no base, or no origin/HEAD.
pub(crate) fn of(s: &Store, r: &Repo, uid: &str) -> Option<Moved> {
    let main = git(&r.root, &["rev-parse", "--abbrev-ref", "origin/HEAD"])?;
    let live = s.live_runs(Some(uid)).ok()?.pop()?;
    let log = main_log(&r.root, &main, live.base.as_deref()?)?;
    let ledger = crate::read(r);
    let (title, brief, issues) = s.stale_input(uid).ok()?;
    let stale = crate::chunk_scope::stale_warnings(&title, &brief, &issues, &ledger, uid);
    let shut = s.shut_since(&live.started).ok()?;
    let mut m = moved(&log, &live, &ledger, &shut, &stale);
    m.fetched = fetched_ago(&r.root);
    Some(m)
}

/// Age of the last fetch, from `FETCH_HEAD`'s mtime. None = never fetched here.
// ponytail: FETCH_HEAD is per worktree, so a fetch from a sibling worktree reads as older
fn fetched_ago(root: &Path) -> Option<String> {
    let p = root.join(git(root, &["rev-parse", "--git-path", "FETCH_HEAD"])?);
    let s = std::fs::metadata(p)
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?
        .as_secs();
    Some(match s {
        0..=89 => "just now".into(),
        90..=5399 => format!("{}m ago", s / 60),
        5400..=129_599 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86400),
    })
}

/// What `chunk done` says about drift: empty when nothing moved.
pub(crate) fn done_line(s: &Store, r: &Repo, uid: &str) -> String {
    of(s, r, uid)
        .and_then(|m| m.line())
        .unwrap_or_default()
        .replace("{uid}", uid)
}

/// The board: sets `drift` on every running chunk that has some. `main` = `origin/HEAD`'s
/// branch; the chunks' `stale` is already filled, so call this after it.
pub(super) fn fill(
    s: &Store,
    root: &Path,
    main: Option<&str>,
    ledger: &fael_core::Log,
    chunks: &mut [BoardChunk],
) {
    let (Some(main), Ok(lives)) = (main, s.live_runs(None)) else {
        return;
    };
    let Some(since) = lives.iter().map(|l| l.started.as_str()).min() else {
        return;
    };
    let shut = s.shut_since(since).unwrap_or_default();
    let mut logs: HashMap<&str, Option<Vec<Commit>>> = HashMap::new();
    for l in &lives {
        let Some(base) = l.base.as_deref() else {
            continue;
        };
        let Some(log) = logs
            .entry(base)
            .or_insert_with(|| main_log(root, main, base))
        else {
            continue;
        };
        if let Some(c) = chunks.iter_mut().find(|c| c.uid == l.uid) {
            let n = moved(log, l, ledger, &shut, &c.stale).n();
            c.drift = (n > 0).then_some(n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str, subject: &str, file: &str) -> Commit {
        Commit {
            sha: sha.into(),
            subject: subject.into(),
            files: vec![file.into()],
        }
    }

    #[test]
    fn only_scope_touching_commits_outside_the_chunks_own_pr_count() {
        let log = [
            commit("aaaaaaaaaaaa", "feat: in scope (#1)", "src/a.rs"),
            commit("bbbbbbbbbbbb", "docs: elsewhere (#2)", "README.md"),
            commit("cccccccccccc", "feat: its own (#3)", "src/b.rs"),
        ];
        let live = Live {
            uid: "U".into(),
            started: "2026-01-02T00:00:00.000Z".into(),
            base: None,
            worktree: None,
            pr: Some(3),
            session: None,
            files: vec!["src/".into()],
        };
        let shut = [Shut {
            uid: "V".into(),
            label: Some("v1".into()),
            title: "closed work".into(),
            scope: vec!["src/a.rs".into()],
            at: "2026-01-03T00:00:00.000Z".into(),
        }];
        let m = moved(&log, &live, &fael_core::Log::default(), &shut, &[]);
        assert_eq!(m.behind, 3);
        assert_eq!(m.commits, ["aaaaaaaaa feat: in scope (#1)"]);
        assert_eq!(m.closed, ["chunk V v1 — closed work"]);
        assert_eq!(m.n(), 2);
    }
}
