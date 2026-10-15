//! `fael plan new` · `export` · `apply` · `park|unpark|done` · `repo`: a plan is sections
//! plus chunks in `fael.db`; `export --json` writes the bill `apply` takes back.

use crate::{Args, Repo, repo};
use fael_core::plan::{Bill, PlanRow, Store};
use std::path::PathBuf;
use std::process::ExitCode;

/// The verbs `plan` answers — `show` is `export` by another name.
pub(crate) const VERBS: &[&str] = &[
    "new", "export", "show", "apply", "park", "unpark", "done", "list",
];

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    let r = repo()?;
    if let [new, name, title] = rest
        && new == "new"
    {
        a.only("plan new", &["app", "area"])?;
        return create(a, &r, name, title).map(|()| ExitCode::SUCCESS);
    }
    // `plan show <plan>` is what agents type for export
    let sub = match rest.first().map(String::as_str) {
        Some("show") => Some("export"),
        s => s,
    };
    let flags: &[&str] = match sub {
        Some("export") => &["json", "since", "section"],
        Some("apply") => &["dry-run"],
        _ => &[],
    };
    a.only("plan", flags)?;
    match sub {
        Some("export") if rest.len() == 1 && a.has("since") => feed(&r, &a.one("since").unwrap()),
        Some("export") if rest.len() == 2 && a.has("section") => {
            section(&r, &rest[1], &a.one("section").unwrap_or_default())
        }
        Some("export") if rest.len() <= 2 => {
            export(&r, rest.get(1).map(String::as_str), a.has("json"))
        }
        Some("apply") if rest.len() == 2 => apply(&r, &rest[1], a.has("dry-run")),
        Some(v @ ("park" | "unpark" | "done")) if rest.len() == 2 => state(&r, v, &rest[1]),
        Some("list") if rest.len() == 1 => list(&r),
        _ => Err(
            "rejected: fael plan new <name> \"<title>\" | fael plan list | fael plan export [<plan>] [--json | --section <key>] | fael plan apply <file|-> [--dry-run] | fael plan park|unpark|done <plan> — try 'fael plan --help'"
                .into(),
        ),
    }
    .map(|()| ExitCode::SUCCESS)
}

/// Beside the journal, so every worktree of the clone reads one db; the tree's `.fael/`
/// (or `FAEL_DIR`) without git.
pub(crate) fn db(r: &Repo) -> PathBuf {
    fael_core::plan::db_at(r.journal.as_deref().unwrap_or(&r.fael))
}

/// `new`: the plan row (chunks in the db from the start) and its Goal/Scope/Done/Constraints
/// skeleton sections, no md file; `fael plan apply` fills them.
fn create(a: &Args, r: &Repo, name: &str, title: &str) -> Result<(), String> {
    let app = a.one("app").unwrap_or_default();
    let area = a.one("area").unwrap_or_else(|| "dev".into());
    let now = fael_core::rfc3339(fael_core::now_ms());
    in_repo(r)?;
    let mut s = Store::open(&db(r))?;
    s.new_plan(&app, name, title, &area, &now)?;
    let key = if app.is_empty() {
        name.to_string()
    } else {
        format!("{app}/{name}")
    };
    crate::board::register(r);
    println!("plan {key} created (Goal/Scope/Done/Constraints sections: every brief reads them)");
    println!(
        "fill them: fael plan export {key} --json > bill.json, edit \"sections\", fael plan apply bill.json"
    );
    println!(
        "add chunks: fael chunk add \"<title>\" --plan {key} --brief \"…\" --size S|M|L --model M --scope <paths>"
    );
    Ok(())
}

/// `apply <file|->`: the bill `export --json` wrote, edited (decision fael-board:plan-bill).
fn apply(r: &Repo, from: &str, dry: bool) -> Result<(), String> {
    let text = if from == "-" {
        let mut t = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut t)
            .map(|_| t)
            .map_err(|e| format!("rejected: cannot read stdin: {e}"))?
    } else {
        std::fs::read_to_string(from).map_err(|e| format!("rejected: {from}: {e}"))?
    };
    let bill: Bill = serde_json::from_str(&text).map_err(|e| {
        format!("rejected: not a plan bill (`fael plan export <plan> --json`): {e}")
    })?;
    let mut s = open(r)?;
    let (lines, rev) = s.bill(bill, &fael_core::rfc3339(fael_core::now_ms()), dry)?;
    for l in &lines {
        println!("{l}");
    }
    match (dry, lines.is_empty()) {
        (true, _) => println!("dry run: nothing written"),
        (false, true) => println!("no change (rev {rev})"),
        (false, false) => {
            crate::board::register(r);
            println!("applied (rev {rev})");
        }
    }
    Ok(())
}

/// `park` · `unpark` · `done`: the owner's word on `plan.state`.
fn state(r: &Repo, verb: &str, want: &str) -> Result<(), String> {
    let to = match verb {
        "park" => "parked",
        "unpark" => "active",
        _ => "done",
    };
    let mut s = open(r)?;
    let p = one(&s, want)?;
    let from = s.plan_state(p.id, to, &fael_core::rfc3339(fael_core::now_ms()))?;
    crate::board::register(r);
    println!("plan {} {from} → {to}", key(&p));
    Ok(())
}

/// A dir with no git and no `.fael` is no repo: never point the agent at `plan new` there,
/// and never leave a stray fael.db in it.
pub(crate) fn in_repo(r: &Repo) -> Result<(), String> {
    if r.journal.is_none() && !r.fael.exists() {
        return Err(format!(
            "rejected: no fael.db yet — {} is in no git repo; run fael from the repo the plan lives in",
            r.cwd.display()
        ));
    }
    Ok(())
}

fn open(r: &Repo) -> Result<Store, String> {
    let path = db(r);
    in_repo(r)?;
    if !path.exists() {
        return Err(
            "rejected: no fael.db yet — `fael plan new <name> \"<title>\"` makes the first plan"
                .into(),
        );
    }
    Store::open(&path)
}

/// `app/name`, or `name` at the root — how `export` names a plan.
fn key(p: &PlanRow) -> String {
    if p.app.is_empty() {
        p.name.clone()
    } else {
        format!("{}/{}", p.app, p.name)
    }
}

/// The plans `want` names (`name` or `app/name`), every one when `None`.
fn named(s: &Store, want: Option<&str>) -> Result<Vec<PlanRow>, String> {
    let plans: Vec<PlanRow> = s
        .plans()?
        .into_iter()
        .filter(|p| want.is_none_or(|w| key(p) == w || p.name == w))
        .collect();
    match (want, plans.len()) {
        (Some(w), 0) => Err(format!(
            "rejected: no plan '{w}' in fael.db — `fael board` lists them"
        )),
        (Some(w), n) if n > 1 => Err(format!("rejected: '{w}' names {n} plans — use app/name")),
        _ => Ok(plans),
    }
}

fn one(s: &Store, want: &str) -> Result<PlanRow, String> {
    named(s, Some(want)).map(|mut v| v.remove(0))
}

/// `plan list`: one line per plan in this repo's fael.db — what `export` takes, its state,
/// its title. Agents typed it 7 of 7 times into a reject (2026-10-13..15).
fn list(r: &Repo) -> Result<(), String> {
    for p in open(r)?.plans()? {
        println!("{} · {} · {}", key(&p), p.state, p.title);
    }
    Ok(())
}

/// Every plan, or the one `want` names (`name` or `app/name`), to stdout; `--json` is the
/// bill `apply` takes back, one plan only.
fn export(r: &Repo, want: Option<&str>, json: bool) -> Result<(), String> {
    let s = open(r)?;
    let plans = named(&s, want)?;
    if json {
        let [p] = plans.as_slice() else {
            return Err("rejected: plan export --json names one plan".into());
        };
        let bill = fael_core::plan::export_json(&s, p.id)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&bill).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let docs: Vec<String> = plans
        .iter()
        .map(|p| fael_core::plan::export(&s, p.id))
        .collect::<Result<_, _>>()?;
    print!("{}", docs.join("\n"));
    Ok(())
}

/// `export <plan> --section <key>`: one section of one plan.
fn section(r: &Repo, want: &str, key: &str) -> Result<(), String> {
    let s = open(r)?;
    let text = fael_core::plan::export_section(&s, one(&s, want)?.id, key)?;
    print!("{text}");
    Ok(())
}

/// `fael plan export --since <event-id>`: the change feed, JSON whatever `--json` says.
fn feed(r: &Repo, since: &str) -> Result<(), String> {
    let since = since
        .parse::<i64>()
        .map_err(|_| format!("rejected: --since takes an event id (0 = all), not {since:?}"))?;
    let f = fael_core::plan::feed(&open(r)?, since)?;
    println!("{}", serde_json::to_string(&f).map_err(|e| e.to_string())?);
    Ok(())
}
