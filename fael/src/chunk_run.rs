//! `fael chunk start|done|move` (PLAN-fael-board b2a): claim a chunk and print its brief,
//! finish it with a PR or an `--out` file copied under `<db dir>/out/<uid>/<R>/`, move it to
//! another plan of this db with its uid and history.

use crate::plan::db;
use crate::{Args, Repo, core};
use fael_core::plan::{Here, Start, Store};
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
    let session = std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty() && a.one("run").is_none());
    let o = Start {
        run: a.one("run"),
        client: a.one("client"),
        model: a.one("model"),
        no_worktree: a.has("no-worktree"),
        force: a.has("force"),
        session: session.clone(),
    };
    let mut st = Store::open(&db(r))?.start(uid, &o, here)?;
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
                *id = format!("{} {}{}", row.id, title.unwrap_or_default(), row.text);
            }
        }
    }
    print!("{}", st.text());
    Ok(())
}

pub(crate) fn done(
    a: &Args,
    r: &Repo,
    uid: &str,
    handoff: &str,
    here: &Here,
) -> Result<(), String> {
    a.only("chunk done", &["pr", "out", "run"])?;
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
        .then_some(&mut copy as &mut fael_core::plan::Copy<'_>);
    let mut st = Store::open(&db(r))?;
    st.done(uid, handoff, pr, out, here)?;
    let trunk = base_of(&r.root, here.branch);
    // issue 01M4RRD1K: what the branch touched, not what it declared; a failed write only
    // keeps the declared scope
    let range = trunk.as_deref().map(|b| format!("{b}...HEAD"));
    if let Some(diff) = range.and_then(|g| crate::git(&r.root, &["diff", "--name-only", &g])) {
        let _ = st.claim(uid, &diff.lines().map(String::from).collect::<Vec<_>>());
    }
    let to = if pr.is_some() { "done" } else { "review" };
    println!("chunk {uid} → {to}");
    if let (Some(trunk), Some(_)) = (trunk, pr) {
        conflict_note(&r.root, &trunk, here.branch.unwrap_or_default());
    }
    Ok(())
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
