//! `fael chunk start|done|move` (PLAN-fael-board b2a): claim a chunk and print its brief,
//! finish it with a PR or an `--out` file copied under `<db dir>/out/<uid>/<R>/`, move it to
//! another plan of this db with its uid and history.

use crate::plan::db;
use crate::{Args, Repo, core};
use fael_crew::{Here, Start, Store};
use std::collections::HashSet;
use std::path::Path;

/// `fael chunk start <uid>`: claim it, bind the caller's session, print the brief.
pub(crate) fn start(a: &Args, r: &Repo, uid: &str, here: &Here) -> Result<(), String> {
    a.only(
        "chunk start",
        &["client", "model", "run", "force", "no-worktree"],
    )?;
    // the agent's own start: an agent that ends with `chunk done` in the same turn never
    // reaches a Stop that could stamp it (issue 01M4NXDQ). Not a launcher (`--run R`): it
    // may sit in another agent's session, and the started agent's SessionStart stamps it
    // ponytail: Claude Code's session env only; another client's when it has one
    // `FAEL_SESSION` first (the OpenCode shell.env hook sets it per command, fresher
    // than an inherited client var — session.rs `env_session` order).
    let session = ["FAEL_SESSION", "CLAUDE_CODE_SESSION_ID", "CODEX_THREAD_ID"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty() && a.one("run").is_none());
    let o = Start {
        run: a.one("run"),
        client: a.one("client"),
        model: a.one("model"),
        no_worktree: a.has("no-worktree"),
        force: a.has("force"),
        session: session.clone(),
        base: crate::git(&r.root, &["rev-parse", "HEAD"]),
        shared: fael_crew::shared(&r.root).unwrap_or_default(),
        stale: crate::board::clock().1,
    };
    let mut store = Store::open(&db(r))?;
    if !o.force {
        crate::chunk_scope::gate(&store, r, uid)?;
    }
    let mut st = store.start(uid, &o, here)?;
    if let Some(m) = session.as_deref().and_then(crate::chunk::held) {
        // the Stop hook finds this db from any cwd; a failed write only skips stamps
        let _ = m.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(m, db(r).to_string_lossy().as_bytes());
    }
    // each linked issue's id → its row, so the agent reads what to fix without a find
    if st.chunks.iter().any(|c| !c.issues.is_empty()) {
        let log = crate::read(r);
        for id in st.chunks.iter_mut().flat_map(|c| &mut c.issues) {
            if let Ok(row) = core::resolve_row(&log, id) {
                let title = row.title.as_deref().map(|t| format!("{t}\n"));
                let tag = row.from_report().then_some("(from report, unchecked)\n");
                *id = format!(
                    "{} {}{}{}",
                    row.id,
                    tag.unwrap_or_default(),
                    title.unwrap_or_default(),
                    row.text
                );
            }
        }
    }
    print!("{}", st.text());
    // plan fael-row-retire c3: contracts and open decisions and notes on files under the
    // chunk's scope, verdicts ready — the agent judges, start never blocks. `fael` words them
    // (its session state and said-once outbox), so this asks it.
    {
        let scope: Vec<String> = st
            .chunks
            .iter()
            .filter_map(|c| c.scope.as_deref())
            .flat_map(str::lines)
            .map(String::from)
            .collect();
        print!("{}", crate::fael_bin::held("start", &scope));
    }
    // the declared scope may name a re-export while the work lives elsewhere
    // (th.ts spreads start.ts): warn now, not only at done's outside report.
    if let Ok(s) = Store::open(&db(r)) {
        for c in &st.chunks {
            crate::chunk_scope::warn(&s, r, &c.uid);
        }
    }
    Ok(())
}

/// issue 01M5H3FGB: a fence rejection that names `--run R` gets the whole retype appended,
/// built from the args passed. The run is never picked for the agent.
fn with_retype(
    e: String,
    uid: &str,
    handoff: &str,
    flag: impl Fn(&str) -> Option<String>,
) -> String {
    let Some(run) = e
        .split("pass --run ")
        .nth(1)
        .and_then(|t| t.split_whitespace().next())
    else {
        return e;
    };
    let q = |v: &str| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
    let mut cmd = format!("fael chunk done {uid} {}", q(handoff));
    for f in ["pr", "out", "review", "left"] {
        if let Some(v) = flag(f) {
            cmd += &format!(" --{f} {}", if v.contains(' ') { q(&v) } else { v });
        }
    }
    format!("{e}\nretype: {cmd} --run {run}")
}

pub(crate) fn done(
    a: &Args,
    r: &Repo,
    uid: &str,
    handoff: &str,
    here: &Here,
) -> Result<(), String> {
    a.only("chunk done", &["pr", "out", "run", "review", "left"])?;
    // `12`, `#12`, `owner/repo#12`, `https://github.com/owner/repo/pull/12`: the number
    let raw = a.one("pr");
    let pr = raw
        .as_deref()
        .map(|n| {
            n.trim_end_matches('/')
                .rsplit(['#', '/'])
                .next()
                .unwrap_or("")
                .parse::<i64>()
        })
        .transpose()
        .map_err(|_| "rejected: --pr takes a PR number, owner/repo#N or its URL")?;
    // issue 01M4QFJ6V: another repo's PR is refused, never dropped; no origin = no check
    if let Some((raw, want)) = raw.as_deref().and_then(|p| Some((p, pr_repo(p)?)))
        && let Some(have) = crate::board::remote(&r.root).filter(|h| !h.eq_ignore_ascii_case(&want))
    {
        return Err(format!("rejected: --pr {raw} is not this repo's ({have})"));
    }
    let src = a.one("out").map(|o| r.cwd.join(o));
    if pr.is_some() && src.is_some() {
        return Err("rejected: --pr or --out, not both".into());
    }
    if let Some(p) = src.as_ref().filter(|p| !p.exists()) {
        return Err(format!("rejected: --out {} does not exist", p.display()));
    }
    let base = db(r).with_file_name("out").join(uid);
    let mut copy = |run: &str| -> Result<String, String> {
        let src = src.as_deref().unwrap_or(Path::new(""));
        let to = base.join(run).join(src.file_name().unwrap_or_default());
        copy_all(src, &to).map_err(|e| format!("--out: copy to {}: {e}", to.display()))?;
        Ok(to.to_string_lossy().into_owned())
    };
    let out = src
        .is_some()
        .then_some(&mut copy as &mut fael_crew::Copy<'_>);
    let mut st = Store::open(&db(r))?;
    // issue 01M5FMHQX: leftovers are rows; the statement rides in the hand-off the next brief prints
    let handoff = &crate::chunk_left::resolve(a.one("left"), handoff, &st, &crate::read(r))?;
    // issue 01M4TFB5: the run's branch, not the cwd's — a lead's done per lane from the wave
    // branch diffs that lane only. Read before done: ending the run from its own worktree
    // stamps the cwd's branch on it, and a lead merges there on the wave branch
    // issue 01M4TPHM: a run started on the trunk, then switched, has no branch of its own
    // recorded — the cwd's is the work
    let head = crate::git(&r.root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]);
    let on_trunk = |b: &String| head.as_deref().and_then(|h| h.strip_prefix("origin/")) == Some(b);
    let branch = st.run_branch(uid, here.run).filter(|b| !on_trunk(b));
    // a run `run end` ended kept its lane's diff then: the lane may be rebased since
    let kept = st.claimed_at_end(uid, here.run);
    // the run's start, read before done ends it: held rows `start` named after it are not repeated
    let started = st.run_started(uid, here.run);
    st.done_review(uid, handoff, pr, out, a.one("review").as_deref(), here)
        .map_err(|e| with_retype(e, uid, handoff, |f| a.one(f)))?;
    let trunk = base_of(&r.root, branch.as_deref().or(here.branch));
    // issue 01M4RRD1K: what the branch touched, not what it declared; a failed write only
    // keeps the declared scope
    let diff = kept.or_else(|| touched(&r.root, &st, uid, branch.as_deref().or(here.branch)));
    let outside = diff.as_ref().map(|d| outside_text(r, &st, uid, d));
    if let Some(d) = &diff {
        let _ = st.claim(uid, d);
    }
    let to = if pr.is_some() { "done" } else { "review" };
    println!("chunk {uid} → {to}");
    print!("{}", outside.unwrap_or_default());
    // plan fael-row-retire w2: open decisions and notes on what the chunk touched, verdicts
    // ready — the agent judges, done never closes or bumps
    if let Some(d) = &diff {
        print!("{}", crate::fael_bin::held_done(d, started.as_deref()));
    }
    if let (Some(trunk), Some(_)) = (trunk, pr) {
        conflict_note(&r.root, &trunk, here.branch.unwrap_or_default());
    }
    Ok(())
}

/// What `uid`'s run on `branch` touched (None = the cwd's HEAD): its trunk diff since the
/// run's start; None = on the trunk or no `origin/HEAD`.
pub(crate) fn touched(
    dir: &Path,
    st: &Store,
    uid: &str,
    branch: Option<&str>,
) -> Option<Vec<String>> {
    let trunk = base_of(dir, branch)?;
    let tip = branch.unwrap_or("HEAD");
    let d = crate::git(dir, &["diff", "--name-only", &format!("{trunk}...{tip}")])?;
    Some(since(dir, st.run_base(uid), (&trunk, tip), d.lines()))
}

/// h4: each path of `diff` outside `uid`'s declared scope, unowned or overlapping the open chunk that
/// holds it — a fact, never a refusal; empty when all is inside. `done` and `run end` say it.
pub(crate) fn outside_text(r: &Repo, st: &Store, uid: &str, diff: &[String]) -> String {
    let shared = fael_crew::shared(&r.root).unwrap_or_default();
    let Some((declared, out)) = st
        .outside(uid, diff, &shared)
        .ok()
        .filter(|(_, o)| !o.is_empty())
    else {
        return String::new();
    };
    let mut s = format!("declared scope: {}\n", declared.join(", "));
    for (f, owner) in out {
        match owner {
            Some(o) => s += &format!("outside declared scope: {f} (overlaps open chunk {o})\n"),
            None => s += &format!("outside declared scope: {f}\n"),
        }
    }
    s
}

/// Issue 01M4TVDGG: of the trunk diff, only the paths changed since the run's start commit —
/// a lane stacked on another lane's branch never claims that lane's files, and a rebase onto
/// the trunk since start never claims the trunk's (the trunk diff cuts them). No base
/// recorded: the trunk diff as is; nothing since the base: nothing claimed.
/// Issue 01M4W673: a lane rebased with the stack it started on (the trunk moved) has a base
/// that is no ancestor any more — the stack's copies keep author, date and subject of a commit
/// behind the base and drop (a patch-id match misses a copy whose context the trunk moved);
/// the trunk's own commits a rebase brought in are not the lane's either.
fn since<'a>(
    dir: &Path,
    base: Option<String>,
    (trunk, tip): (&str, &str),
    diff: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let Some(b) = base else {
        return diff.map(String::from).collect();
    };
    let (range, not) = (format!("{b}...{tip}"), format!("^{trunk}"));
    let log = |side: &str, more: &[&str]| {
        let head = ["log", side, "--format=%x01%ae %at %s"];
        let args = [&head[..], more, &[&range, &not]].concat();
        crate::git(dir, &args).unwrap_or_default()
    };
    let behind = log("--left-only", &[]);
    let behind: HashSet<&str> = behind.split('\x01').map(str::trim).collect();
    let ahead = log("--right-only", &["--name-only"]);
    let since: HashSet<&str> = ahead
        .split('\x01')
        .filter_map(|c| c.split_once('\n'))
        .filter(|(key, _)| !behind.contains(key.trim()))
        .flat_map(|(_, files)| files.lines())
        .collect();
    diff.filter(|f| since.contains(f))
        .map(String::from)
        .collect()
}

/// `owner/repo` of `owner/repo#12` or `…/owner/repo/pull/12`; None for `12` or `#12`.
fn pr_repo(pr: &str) -> Option<String> {
    let pr = pr.trim_end_matches('/');
    let head = match pr.split_once('#') {
        Some((h, _)) => h,
        None => pr.rsplit_once("/pull/")?.0,
    };
    let mut parts = head.rsplit('/').filter(|s| !s.is_empty());
    let name = parts.next()?;
    Some(format!("{}/{name}", parts.next()?))
}

/// The base (`origin/HEAD`, no fetch, as board does) when `branch` is another branch.
fn base_of(dir: &Path, branch: Option<&str>) -> Option<String> {
    let base = crate::git(dir, &["rev-parse", "--abbrev-ref", "origin/HEAD"])?;
    branch.filter(|b| Some(*b) != base.strip_prefix("origin/"))?;
    Some(base)
}

/// Decision 01M4QJPX: a PR whose branch no longer merges into the base is told which
/// files — never refused; a git error says nothing.
fn conflict_note(dir: &Path, base: &str, b: &str) {
    let files = crate::board::prs::merge_tree(dir, base, "HEAD").unwrap_or_default();
    if !files.is_empty() {
        let files = files.join(", ");
        println!("note: {b} conflicts with {base} in {files} — rebase before review");
    }
}

/// `fael chunk move <uid> --plan <p>`: the chunk goes to the end of `p`, uid, runs, notes
/// and edges kept.
pub(crate) fn move_to(a: &Args, s: &mut Store, uid: &str, now: &str) -> Result<(), String> {
    a.only("chunk move", &["plan"])?;
    let plan = a
        .one("plan")
        .ok_or("rejected: chunk move needs --plan <p>")?;
    let from = s.move_to(uid, &plan, now)?;
    println!("chunk {uid} → moved from {from} to {plan}");
    Ok(())
}

/// A file or a whole dir. `fs::copy` clones on APFS, so a big video costs no space.
fn copy_all(src: &Path, to: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_all(&e.path(), &to.join(e.file_name()))?;
        }
        return Ok(());
    }
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::copy(src, to).map(|_| ())
}

#[cfg(test)]
mod retype_tests {
    use super::with_retype;

    #[test]
    fn a_fence_reject_gets_a_copyable_retype_and_other_errors_stay() {
        let e = "rejected: chunk C1 was taken over by run R9 (codex) in /w — pass --run R9";
        let flag = |f: &str| match f {
            "pr" => Some("12".to_string()),
            "left" => Some("none".to_string()),
            _ => None,
        };
        let got = with_retype(e.into(), "C1", "said \"hi\"", flag);
        assert!(got.starts_with(e), "{got}");
        assert!(
            got.ends_with(
                "retype: fael chunk done C1 \"said \\\"hi\\\"\" --pr 12 --left none --run R9"
            ),
            "{got}"
        );
        assert_eq!(
            with_retype("rejected: x".into(), "C1", "h", flag),
            "rejected: x"
        );
    }
}
