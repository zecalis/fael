//! `fael chunk …` and `fael run end R` (PLAN-fael-board b2a, SPEC §1): the CLI over the
//! chunk contract in `fael_core::plan`. The worktree is this repo's root; the db is the
//! clone's `fael.db`; `--out` copies under `<db dir>/out/<uid>/<R>/`.

use crate::plan::db;
use crate::{Args, Repo, core, repo};
use fael_core::plan::{Fields, Here, Owner, Store};
use std::process::ExitCode;

const USAGE: &str = "fael chunk add|show|start|edit|move|note|wait|after|ask|reply|answer|done|accept|drop|park|unpark|apply|approve|unapprove — try 'fael chunk --help'";

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    let r = repo()?;
    let named;
    let (a, rest) = match in_plan(a, rest, &r).map_err(|e| row_id(&r, e))? {
        Some(n) => {
            named = n;
            (&named.0, named.1.as_slice())
        }
        None => (a, rest),
    };
    verb(a, rest, &r).map_err(|e| row_id(&r, e))
}

/// `chunk show b1 --plan p`: on a verb that names a chunk (not `add` or `move`, where
/// `--plan` is where it goes), the plan picks it — the key becomes its uid, the flag goes.
fn in_plan(a: &Args, rest: &[String], r: &Repo) -> Result<Option<(Args, Vec<String>)>, String> {
    let (Some(plan), [verb, key, ..]) = (a.one("plan"), rest) else {
        return Ok(None);
    };
    if matches!(verb.as_str(), "add" | "move" | "apply") {
        return Ok(None);
    }
    let uid = Store::open(&db(r))?.uid_in(key, &plan)?;
    let mut rest = rest.to_vec();
    rest[1] = uid;
    Ok(Some((a.without("plan"), rest)))
}

/// `chunk show 01M4P3R3` with a ledger row's id: say it is a row, not "no chunk".
fn row_id(r: &Repo, e: String) -> String {
    let Some(key) = e
        .strip_prefix("rejected: no chunk ")
        .and_then(|t| t.split(' ').next())
    else {
        return e;
    };
    match core::resolve_row(&crate::read(r), key) {
        Ok(row) => format!(
            "rejected: {key} is a ledger {}, not a chunk — `fael find {key}` reads it, `fael board` lists the chunk uids",
            row.kind
        ),
        Err(_) => e,
    }
}

fn verb(a: &Args, rest: &[String], r: &Repo) -> Result<ExitCode, String> {
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
    let s = || Store::open(&db(r));
    let say = |uid: &str, what: &str| println!("chunk {uid} → {what}");
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    match rest.as_slice() {
        ["show", key] => return crate::chunk_show::show(a, r, key),
        ["add", title] => {
            a.only(
                "chunk add",
                &[
                    "plan", "brief", "size", "model", "scope", "reads", "after", "label", "issue",
                ],
            )?;
            let f = Fields {
                title: Some(title.to_string()),
                issues: issues(r, &list(a, "issue"))?,
                ..fields(a)
            };
            let plan = a.one("plan").unwrap_or_else(|| "inbox".into());
            let mut st = s()?;
            let uid = st.add(&plan, &f, &list(a, "after"), &now)?;
            println!("{uid}");
            crate::chunk_scope::warn(&st, &r.root, &uid);
        }
        ["start", uid] => crate::chunk_run::start(a, r, uid, &here)?,
        ["edit", uid] => {
            a.only(
                "chunk edit",
                &["title", "brief", "size", "model", "scope", "reads", "label"],
            )?;
            let mut st = s()?;
            st.edit(uid, &fields(a), &now)?;
            say(uid, "edited");
            if a.has("scope") {
                crate::chunk_scope::warn(&st, &r.root, uid);
            }
        }
        ["move", uid] => crate::chunk_run::move_to(a, &mut s()?, uid, &now)?,
        ["note", uid, text] => {
            a.only("chunk note", &[])?;
            s()?.note(uid, text, &now)?;
            say(uid, "noted for the next start");
        }
        ["wait", uid, text] => {
            // `--run R`: a start in another checkout's worktree (`start --worktree`) answers here
            a.only("chunk wait", &["on", "until", "option", "run"])?;
            let on = a
                .one("on")
                .ok_or("rejected: chunk wait needs --on owner|data")?;
            let (until, options) = (a.one("until"), a.many("option"));
            s()?.wait(uid, &on, text, until.as_deref(), &options, &here)?;
            say(uid, &format!("waiting on {on}"));
        }
        ["after", uid, other, why] => {
            a.only("chunk after", &["run"])?;
            s()?.after(uid, other, why, &here)?;
            say(uid, &format!("after {other}"));
        }
        ["done", uid, handoff] => crate::chunk_run::done(a, r, uid, handoff, &here)?,
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
        ["apply" | "approve" | "unapprove", ..] => queue(a, &rest, &mut s()?, &now)?,
        _ => return Err(format!("rejected: {USAGE}")),
    }
    crate::board::register(r);
    Ok(ExitCode::SUCCESS)
}

/// The planner's batch and the owner's queue mark: `apply`, `approve` and its undo.
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

/// `<state>/held/<session>`: the fael.db a session's `chunk start` bound a run in, so its
/// Stop hook stamps that run from any cwd (a `--no-worktree` run has no worktree to match).
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
        scope: a.has("scope").then(|| list(a, "scope")),
        reads: a.has("reads").then(|| list(a, "reads")),
        label: a.one("label"),
        issues: vec![],
    }
}

/// `--issue a,b`: each an open issue row of this repo's ledger (a unique prefix will do),
/// as its full id — the ledger is not in fael.db, so the check is here.
fn issues(r: &Repo, ids: &[String]) -> Result<Vec<String>, String> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let log = crate::read(r);
    let open = core::find(&log, &core::Filter::default());
    ids.iter()
        .map(|id| {
            let row = core::resolve_row(&log, id)?;
            if row.kind != "issue" {
                return Err(format!(
                    "rejected: {} is a {}, not an issue",
                    row.id, row.kind
                ));
            }
            if !open.iter().any(|o| o.id == row.id) {
                return Err(format!(
                    "rejected: issue {} is closed or superseded — link an open one (fael find)",
                    row.id
                ));
            }
            Ok(row.id.clone())
        })
        .collect()
}

/// `fael kickoff PLAN-x.md` (anchor `plan:x`): the chunk this worktree holds, else the next
/// ready one of plan `x` — `fael chunk start` prints its brief and the rules.
pub(crate) fn kickoff(r: &Repo, anchor: &str) {
    let name = anchor.split_once(':').map_or(anchor, |(_, n)| n);
    let path = db(r);
    let Some(s) = path.exists().then(|| Store::open(&path).ok()).flatten() else {
        return;
    };
    let Some(p) = s
        .plans()
        .ok()
        .into_iter()
        .flatten()
        .find(|p| p.name == name)
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
