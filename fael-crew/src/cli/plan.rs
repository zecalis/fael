//! `fael plan new` · `export` · `apply` · `park|unpark|done` · `repo`: a plan is sections
//! plus chunks in `crew.db`; `export --json` writes the bill `apply` takes back.

use crate::{Args, Repo, repo};
use fael_crew::{Bill, PlanRow, Store};
use std::path::PathBuf;
use std::process::ExitCode;

const PLAN_USAGE: &str = "fael plan new <name> \"<title>\" [--app A] [--area dev|marketing] [--parent <plan>] | fael plan list | fael plan export [<plan>] [--json | --section <key>] | fael plan apply <file|-> [--dry-run] | fael plan park|unpark|done <plan> | fael plan lint <plan> — try 'fael plan --help'";

/// The verbs `plan` answers — `show` is `export` by another name.
/// Canonical in `fael-core::work`; the command reuses it.
pub(crate) const VERBS: &[&str] = fael_core::work::PLAN_VERBS;

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    if rest.is_empty() {
        println!("{}", fael_core::help::bare("plan").unwrap_or_default());
        return Ok(ExitCode::SUCCESS);
    }
    let r = repo()?;
    if let [new, name, title] = rest
        && new == "new"
    {
        a.only("plan new", &["app", "area", "parent"])?;
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
        Some("edit") => &["section", "replace", "with"],
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
        Some("edit") if rest.len() == 2 => edit(a, &r, &rest[1]),
        Some(v @ ("park" | "unpark" | "done")) if rest.len() == 2 => state(&r, v, &rest[1]),
        Some("list") if rest.len() == 1 => list(&r),
        Some("lint") if rest.len() == 2 => return lint(&r, &rest[1]),
        _ => Err(fael_core::help::wrong_args(
            "plan",
            rest.first()
                .map(String::as_str)
                .filter(|v| VERBS.contains(v)),
            PLAN_USAGE,
        )),
    }
    .map(|()| ExitCode::SUCCESS)
}

/// Beside the journal, so every worktree of the clone reads one db; the tree's `.fael/`
/// (or `FAEL_DIR`) without git.
pub(crate) fn db(r: &Repo) -> PathBuf {
    fael_crew::db_at(r.journal.as_deref().unwrap_or(&r.fael))
}

/// `new`: the plan row (chunks in the db from the start) and its Goal/Scope/Done/Constraints
/// skeleton sections, no md file; `fael plan apply` fills them.
fn create(a: &Args, r: &Repo, name: &str, title: &str) -> Result<(), String> {
    let app = a.one("app").unwrap_or_default();
    let area = a.one("area").unwrap_or_else(|| "dev".into());
    let now = fael_core::rfc3339(fael_core::now_ms());
    in_repo(r)?;
    let mut s = Store::open(&db(r))?;
    s.new_plan(&app, name, title, &area, a.one("parent").as_deref(), &now)?;
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
        "add chunks: fael chunk add \"<title>\" --plan {key} --brief \"…\" --size S|M|L --model small|large --scope <paths>"
    );
    Ok(())
}

/// `edit <plan> --section <key> --replace <old> --with <new>`: one passage of a section,
/// through the same bill as `apply` (issue 01M5D5FCG).
fn edit(a: &Args, r: &Repo, want: &str) -> Result<(), String> {
    let (Some(key), Some(old)) = (a.one("section"), a.one("replace")) else {
        return Err(
            "rejected: plan edit needs --section <key> --replace <old> --with <new>".into(),
        );
    };
    let new = a
        .one("with")
        .ok_or("rejected: --replace needs --with \"<new text>\"")?;
    let mut s = open(r)?;
    let p = one(&s, want)?;
    let now = fael_core::rfc3339(fael_core::now_ms());
    let (_, rev) = s.plan_edit(&p, &key, &old, &new, &now)?;
    crate::board::register(r);
    println!("applied (rev {rev})");
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
            for uid in changed_uids(&lines) {
                crate::chunk_scope::warn(&s, r, &uid);
            }
            crate::board::register(r);
            println!("applied (rev {rev})");
        }
    }
    Ok(())
}

/// Uids of the chunks a bill added or changed: `+`/`~` lines carry the uid second,
/// merge/split lines carry the new ones past `→`. Drops (`- `) are history, not warned.
fn changed_uids(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let push = |tok: &str, out: &mut Vec<String>| {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric());
        if fael_core::looks_like_id(t) && !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    };
    for l in lines {
        if let Some(rest) = l.strip_prefix("+ ").or_else(|| l.strip_prefix("~ ")) {
            if let Some(uid) = rest.split_whitespace().next() {
                push(uid, &mut out);
            }
        } else if let Some((_, right)) = l.split_once('→') {
            for tok in right.split(|c: char| c.is_whitespace() || c == ',') {
                push(tok, &mut out);
            }
        }
    }
    out
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
/// and never leave a stray crew.db in it.
pub(crate) fn in_repo(r: &Repo) -> Result<(), String> {
    if r.journal.is_none() && !r.fael.exists() {
        return Err(format!(
            "rejected: no crew.db yet — {} is in no git repo; run fael from the repo the plan lives in",
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
            "rejected: no crew.db yet — `fael plan new <name> \"<title>\"` makes the first plan"
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
            "rejected: no plan '{w}' in crew.db — `fael board` lists them"
        )),
        (Some(w), n) if n > 1 => Err(format!("rejected: '{w}' names {n} plans — use app/name")),
        _ => Ok(plans),
    }
}

fn one(s: &Store, want: &str) -> Result<PlanRow, String> {
    named(s, Some(want)).map(|mut v| v.remove(0))
}

/// `plan list`: one line per plan in this repo's crew.db — what `export` takes, its state,
/// its title. Agents typed it 7 of 7 times into a reject (2026-10-13..15).
fn list(r: &Repo) -> Result<(), String> {
    for p in open(r)?.plans()? {
        let up = p
            .parent
            .as_ref()
            .map(|u| format!(" · parent {u}"))
            .unwrap_or_default();
        println!("{} · {} · {}{up}", key(&p), p.state, p.title);
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
        let bill = fael_crew::export_json(&s, p.id)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&bill).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let docs: Vec<String> = plans
        .iter()
        .map(|p| fael_crew::export(&s, p.id))
        .collect::<Result<_, _>>()?;
    print!("{}", docs.join("\n"));
    Ok(())
}

/// `export <plan> --section <key>`: one section of one plan.
fn section(r: &Repo, want: &str, key: &str) -> Result<(), String> {
    let s = open(r)?;
    let text = fael_crew::export_section(&s, one(&s, want)?.id, key)?;
    print!("{text}");
    Ok(())
}

/// `fael plan lint <plan>`: section-cited H-refs and label-like tokens with
/// no chunk row (issue 01M5KVYE). Clean exits 0; drift exits 1 with the fix.
fn lint(r: &Repo, want: &str) -> Result<ExitCode, String> {
    let s = open(r)?;
    let p = one(&s, want)?;
    let missing = s.plan_lint(p.id)?;
    if missing.is_empty() {
        println!("plan {}: no dangling refs", key(&p));
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "plan {}: {} {} with no chunk row",
        key(&p),
        missing.len(),
        if missing.len() == 1 { "ref" } else { "refs" }
    );
    for m in &missing {
        println!(
            "  {} (cited in {}) — no chunk with label {}",
            m.name,
            m.sections.join(", "),
            m.name
        );
    }
    println!(
        "fix: `fael chunk add \"<title>\" --plan {} --label <ref> --brief \"…\"` or edit the section (`fael plan export {} --section <key>`)",
        key(&p),
        key(&p)
    );
    Ok(ExitCode::from(1))
}

/// `fael plan export --since <event-id>`: the change feed, JSON whatever `--json` says.
fn feed(r: &Repo, since: &str) -> Result<(), String> {
    let since = since
        .parse::<i64>()
        .map_err(|_| format!("rejected: --since takes an event id (0 = all), not {since:?}"))?;
    let f = fael_crew::feed(&open(r)?, since)?;
    println!("{}", serde_json::to_string(&f).map_err(|e| e.to_string())?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::changed_uids;

    #[test]
    fn apply_warns_the_chunks_it_added_or_changed() {
        let a = "01M00000000000000000000AA";
        let b = "01M00000000000000000000BB";
        let c = "01M00000000000000000000CC";
        let d = "01M00000000000000000000DD";
        let e = "01M00000000000000000000EE";
        let lines = vec![
            format!("+ {a} new chunk (open)"),
            format!("~ {b} edited brief"),
            format!("- {c} dropped"),
            "plan inbox: edited title".to_string(),
            format!("merged {b}, {c} → {d}"),
            format!("split {b} → {d}, {e}"),
            format!("paired {a}, {b}"),
            format!("{a} after {b}"),
        ];
        // adds, edits and the new chunks past → warn; drops and pure after/pair lines do not
        assert_eq!(changed_uids(&lines), vec![a, b, d, e]);
    }
}
