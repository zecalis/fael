//! `fael chunk …` and `fael run end R` (PLAN-fael-board b2a, SPEC §1): the CLI over the
//! chunk contract in `fael_core::plan`. The worktree is this repo's root; the db is the
//! clone's `plans.db`; `--out` copies under `<db dir>/out/<uid>/<R>/`.

use crate::plan::db;
use crate::{Args, Repo, repo};
use fael_core::plan::{Fields, Here, Owner, Start, Store};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "fael chunk add|show|start|edit|note|wait|after|ask|reply|answer|done|accept|drop|park|unpark|apply|pin|unpin|approve|unapprove — try 'fael chunk --help'";

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    let r = repo()?;
    let now = fael_core::rfc3339(fael_core::now_ms());
    let wt = r.root.to_string_lossy().into_owned();
    let branch = crate::journal::head_branch(&r.root);
    let run = a.one("run");
    let here = Here {
        worktree: &wt,
        branch: branch.as_deref(),
        now: &now,
        run: run.as_deref(),
    };
    let s = || Store::open(&db(&r));
    let say = |uid: &str, what: &str| println!("chunk {uid} → {what}");
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    match rest.as_slice() {
        ["show", key] => return crate::chunk_show::show(a, &r, key),
        [_, ..] if s().is_ok_and(|s| crate::chunk_ask::takes(&rest, &s)) => {
            crate::chunk_ask::run(a, &rest, &mut s()?, &here)?
        }
        ["add", title] => {
            a.only(
                "chunk add",
                &[
                    "plan", "brief", "size", "model", "client", "scope", "after", "label",
                ],
            )?;
            let f = Fields {
                title: Some(title.to_string()),
                ..fields(a)
            };
            let plan = a.one("plan").unwrap_or_else(|| "inbox".into());
            println!("{}", s()?.add(&plan, &f, &list(a, "after"), &now)?);
        }
        ["start", uid] => start(a, &r, uid, &here)?,
        ["edit", uid] => {
            a.only(
                "chunk edit",
                &[
                    "title", "brief", "size", "model", "client", "scope", "label",
                ],
            )?;
            s()?.edit(uid, &fields(a), &now)?;
            say(uid, "edited");
        }
        ["note", uid, text] => {
            a.only("chunk note", &[])?;
            s()?.note(uid, text, &now)?;
            say(uid, "noted for the next start");
        }
        ["wait", uid, text] => {
            a.only("chunk wait", &["on", "until"])?;
            let on = a
                .one("on")
                .ok_or("rejected: chunk wait needs --on owner|data")?;
            s()?.wait(uid, &on, text, a.one("until").as_deref(), &here)?;
            say(uid, &format!("waiting on {on}"));
        }
        ["after", uid, other, why] => {
            a.only("chunk after", &[])?;
            s()?.after(uid, other, why, &here)?;
            say(uid, &format!("after {other}"));
        }
        ["done", uid, handoff] => done(a, &r, uid, handoff, &here)?,
        [verb @ ("answer" | "drop" | "park"), uid, text] => {
            a.only(&format!("chunk {verb}"), &[])?;
            let (cmd, to) = owner(verb);
            s()?.owner(uid, cmd, Some(text), &now)?;
            say(uid, to);
        }
        [verb @ ("accept" | "unpark"), uid] => {
            a.only(&format!("chunk {verb}"), &[])?;
            let (cmd, to) = owner(verb);
            s()?.owner(uid, cmd, None, &now)?;
            say(uid, to);
        }
        ["apply" | "pin" | "unpin" | "approve" | "unapprove", ..] => {
            queue(a, &rest, &mut s()?, &now)?
        }
        _ => return Err(format!("rejected: {USAGE}")),
    }
    // the db already holds the change: a stale md mirror is said, never a failed command
    if let Err(e) = s().and_then(|s| crate::plan::mirror(&r, &s)) {
        eprintln!("fael: md mirror not written: {e}");
    }
    crate::board::register(&r);
    Ok(ExitCode::SUCCESS)
}

/// `fael chunk start <uid>`: claim it, bind the caller's session, print the brief.
fn start(a: &Args, r: &Repo, uid: &str, here: &Here) -> Result<(), String> {
    a.only(
        "chunk start",
        &["client", "model", "run", "force", "no-worktree"],
    )?;
    // only an interactive `--no-worktree` start: a worktree run is stamped by its worktree,
    // and a launcher (`--run R`) may sit in another agent's session, not the one it starts
    // ponytail: Claude Code's session env only; another client's when it has one
    let session = std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty() && a.has("no-worktree") && a.one("run").is_none());
    let o = Start {
        run: a.one("run"),
        client: a.one("client"),
        model: a.one("model"),
        no_worktree: a.has("no-worktree"),
        force: a.has("force"),
        session: session.clone(),
    };
    let st = Store::open(&db(r))?.start(uid, &o, here)?;
    if let Some(m) = session.as_deref().and_then(held) {
        // the Stop hook finds this db from any cwd; a failed write only skips stamps
        let _ = m.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(m, db(r).to_string_lossy().as_bytes());
    }
    let md = |src: &str| std::fs::read_to_string(r.root.join(src)).ok();
    let exists = |p: &str| r.root.join(p).exists();
    print!("{}", st.text(&md, &exists));
    Ok(())
}

/// The planner's batch and the owner's queue marks: `apply`, `pin`, `approve` and their undo.
fn queue(a: &Args, rest: &[&str], s: &mut Store, now: &str) -> Result<(), String> {
    let say = |uid: &str, what: &str| println!("chunk {uid} → {what}");
    match rest {
        ["apply", "-"] if a.has("json") => {
            a.only("chunk apply", &["json"])?;
            let mut stdin = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin)
                .map_err(|e| format!("rejected: cannot read stdin: {e}"))?;
            let ops: Vec<fael_core::plan::Op> = serde_json::from_str(&stdin).map_err(|e| {
                format!("rejected: stdin is not a JSON array of merge/split/pair/after ops: {e}")
            })?;
            for line in s.apply(ops, now)? {
                println!("{line}");
            }
        }
        ["pin", uid, rank] => {
            a.only("chunk pin", &[])?;
            let n = rank
                .parse::<i64>()
                .map_err(|_| "rejected: chunk pin <uid> <rank> — a whole number, lowest first")?;
            s.pin(uid, Some(n), now)?;
            say(uid, &format!("pinned {n}"));
        }
        ["unpin", uid] => {
            a.only("chunk unpin", &[])?;
            s.pin(uid, None, now)?;
            say(uid, "unpinned");
        }
        ["approve", uid] => {
            a.only("chunk approve", &["model", "client"])?;
            let (Some(m), Some(c)) = (a.one("model"), a.one("client")) else {
                return Err("rejected: chunk approve needs --model M --client C".into());
            };
            s.approve(uid, Some((&m, &c)), now)?;
            say(uid, &format!("approved · model {m} · client {c}"));
        }
        ["unapprove", uid] => {
            a.only("chunk unapprove", &[])?;
            s.approve(uid, None, now)?;
            say(uid, "unapproved");
        }
        _ => return Err(format!("rejected: {USAGE}")),
    }
    Ok(())
}

/// `fael run end R [--cost USD]`: ends start R's live rows, nothing else; none left is fine
/// (exit 0). `--cost` is what the client reported (`total_cost_usd`); fael never prices.
pub(crate) fn run_end(a: &Args, run: &str) -> Result<ExitCode, String> {
    a.only("run end", &["cost"])?;
    let cost = match a.one("cost") {
        None => None,
        Some(v) => match v.parse::<f64>() {
            Ok(c) if c.is_finite() && c >= 0.0 => Some(c),
            _ => return Err(format!("rejected: --cost {v} — a decimal >= 0, in USD")),
        },
    };
    let r = repo()?;
    let path = db(&r);
    if path.exists() {
        Store::open(&path)?.run_end(run, cost, &fael_core::rfc3339(fael_core::now_ms()))?;
    }
    Ok(ExitCode::SUCCESS)
}

/// `fael run beat R`: start R's live rows are seen now — the launcher calls it while a
/// headless client runs; none left exits 0.
pub(crate) fn run_beat(a: &Args, run: &str) -> Result<ExitCode, String> {
    a.only("run beat", &[])?;
    let path = db(&repo()?);
    if path.exists() {
        Store::open(&path)?.beat(run, &fael_core::rfc3339(fael_core::now_ms()))?;
    }
    Ok(ExitCode::SUCCESS)
}

/// `<state>/held/<session>`: the plans.db a session's `chunk start` bound a run in, so its
/// Stop hook stamps that run from any cwd (a chunk whose code is in another repo).
pub(crate) fn held(session: &str) -> Option<std::path::PathBuf> {
    session
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        .then(|| fael_core::stats::state_dir().join("held").join(session))
}

/// The command and the state it lands in.
fn owner(verb: &str) -> (Owner, &'static str) {
    match verb {
        "answer" => (Owner::Answer, "open"),
        "accept" => (Owner::Accept, "done"),
        "drop" => (Owner::Drop, "dropped"),
        "park" => (Owner::Park, "parked"),
        _ => (Owner::Unpark, "open"),
    }
}

/// `--scope a,b --scope c` → [a, b, c]
fn list(a: &Args, f: &str) -> Vec<String> {
    a.many(f)
        .iter()
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn fields(a: &Args) -> Fields {
    Fields {
        title: a.one("title"),
        brief: a.one("brief"),
        size: a.one("size"),
        model: a.one("model"),
        client: a.one("client"),
        scope: a.has("scope").then(|| list(a, "scope")),
        label: a.one("label"),
    }
}

fn done(a: &Args, r: &Repo, uid: &str, handoff: &str, here: &Here) -> Result<(), String> {
    a.only("chunk done", &["pr", "out", "run"])?;
    // `12`, `#12`, `owner/repo#12`, `https://github.com/owner/repo/pull/12`: the number;
    // the repo is the plan's (`fael plan repo`), so a link resolves there
    let pr = a
        .one("pr")
        .map(|n| {
            n.trim_end_matches('/')
                .rsplit(['#', '/'])
                .next()
                .unwrap_or("")
                .parse::<i64>()
        })
        .transpose()
        .map_err(|_| "rejected: --pr takes a PR number, owner/repo#N or its URL")?;
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
    Store::open(&db(r))?.done(uid, handoff, pr, out, here)?;
    let to = if pr.is_some() { "done" } else { "review" };
    println!("chunk {uid} → {to}");
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

/// `fael kickoff PLAN-x.md` on a `db` plan: the chunk this worktree holds, else the next
/// ready one — `fael chunk start` prints its brief and the rules.
pub(crate) fn kickoff(r: &Repo, file: &str) {
    let path = db(r);
    let Some(s) = path.exists().then(|| Store::open(&path).ok()).flatten() else {
        return;
    };
    let Some(p) = s
        .plans()
        .ok()
        .into_iter()
        .flatten()
        .find(|p| p.truth == "db" && p.source == file)
    else {
        return;
    };
    let wt = r.root.to_string_lossy();
    if let Some(held) = s.held(&wt).ok().filter(|h| !h.is_empty()) {
        for (uid, title, run) in &held {
            println!("you hold chunk {uid} — {title} (run {run})");
        }
        print!("{}", fael_core::plan::rules(&held[0].0));
        return;
    }
    // ponytail: UTC date, as `chunk start` reads it; a local-day `--until` when the owner asks
    let today = fael_core::rfc3339(fael_core::now_ms());
    match s.first_ready(p.id, &today[..10]) {
        Ok(Some((uid, title, brief))) => {
            println!("next chunk: {uid} — {title}");
            if brief != title {
                println!("> {}", brief.lines().next().unwrap_or_default());
            }
            println!("start it: `fael chunk start {uid}` (prints the brief and the chunk rules)\n");
        }
        Ok(None) => println!(
            "no chunk of {} is ready — `fael plan export {}` shows each state\n",
            p.name, p.name
        ),
        Err(_) => {}
    }
}
