//! `fael chunk …` and `fael run end R` (PLAN-fael-board b2a, SPEC §1): the CLI over the
//! chunk contract in `fael_core::plan`. The worktree is this repo's root; the db is the
//! clone's `plans.db`; `--out` copies under `<db dir>/out/<uid>/<R>/`.

use crate::plan::db;
use crate::{Args, Repo, repo};
use fael_core::plan::{Fields, Here, Owner, Start, Store};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "fael chunk add|start|edit|wait|after|answer|done|accept|drop|park|unpark — try 'fael chunk --help'";

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    let r = repo()?;
    let now = fael_core::rfc3339(fael_core::now_ms());
    let wt = r.root.to_string_lossy().into_owned();
    let branch = crate::journal::head_branch(&r.root);
    let here = Here {
        worktree: &wt,
        branch: branch.as_deref(),
        now: &now,
    };
    let s = || Store::open(&db(&r));
    let say = |uid: &str, what: &str| println!("chunk {uid} → {what}");
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    match rest.as_slice() {
        ["add", title] => {
            a.only(
                "chunk add",
                &["plan", "brief", "size", "model", "scope", "after"],
            )?;
            let f = Fields {
                title: Some(title.to_string()),
                ..fields(a)
            };
            let plan = a.one("plan").unwrap_or_else(|| "inbox".into());
            println!("{}", s()?.add(&plan, &f, &list(a, "after"), &now)?);
        }
        ["start", uid] => {
            a.only("chunk start", &["client", "run", "force"])?;
            let o = Start {
                run: a.one("run"),
                client: a.one("client"),
                force: a.has("force"),
            };
            let st = s()?.start(uid, &o, &here)?;
            let md = |src: &str| std::fs::read_to_string(r.root.join(src)).ok();
            let exists = |p: &str| r.root.join(p).exists();
            print!("{}", st.text(&md, &exists));
        }
        ["edit", uid] => {
            a.only("chunk edit", &["title", "brief", "size", "model", "scope"])?;
            s()?.edit(uid, &fields(a), &now)?;
            say(uid, "edited");
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
        _ => return Err(format!("rejected: {USAGE}")),
    }
    Ok(ExitCode::SUCCESS)
}

/// `fael run end R`: ends start R's live rows, nothing else; none left is fine (exit 0).
pub(crate) fn run_end(a: &Args, run: &str) -> Result<ExitCode, String> {
    a.only("run end", &[])?;
    let r = repo()?;
    let path = db(&r);
    if path.exists() {
        Store::open(&path)?.run_end(run, &fael_core::rfc3339(fael_core::now_ms()))?;
    }
    Ok(ExitCode::SUCCESS)
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
        scope: a.has("scope").then(|| list(a, "scope")),
    }
}

fn done(a: &Args, r: &Repo, uid: &str, handoff: &str, here: &Here) -> Result<(), String> {
    a.only("chunk done", &["pr", "out"])?;
    let pr = a
        .one("pr")
        .map(|n| n.trim_start_matches('#').parse::<i64>())
        .transpose()
        .map_err(|_| "rejected: --pr takes a PR number")?;
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
    println!("chunk {uid} → review");
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
