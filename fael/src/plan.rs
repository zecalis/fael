//! `fael plan import` · `next` · `export` · `cutover` (PLAN-fael-board b1, b1b, c1): read
//! every `.fapony/` of the repo into `plans.db`, print each plan's next chunk from the db —
//! the same pick `fapony plan` makes, so `scripts/plan-parity.py` can hold the two to each
//! other — write a plan back out as markdown, and hand one plan's chunks to the db.

use crate::{Args, Repo, repo};
use fael_core::plan::{Bill, Import, Next, PlanRow, Store, md};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    let r = repo()?;
    if let [new, name, title] = rest
        && new == "new"
    {
        a.only("plan new", &["app", "area"])?;
        return create(a, &r, name, title).map(|()| ExitCode::SUCCESS);
    }
    let flags: &[&str] = match rest.first().map(String::as_str) {
        Some("export") => &["json", "since", "section"],
        Some("apply") => &["dry-run"],
        _ => &[],
    };
    a.only("plan", flags)?;
    match rest.first().map(String::as_str) {
        Some("import") if rest.len() == 1 => import(&r),
        Some("next") if rest.len() == 1 => next(&r),
        Some("export") if rest.len() == 1 && a.has("since") => feed(&r, &a.one("since").unwrap()),
        Some("export") if rest.len() == 2 && a.has("section") => {
            section(&r, &rest[1], &a.one("section").unwrap_or_default())
        }
        Some("export") if rest.len() <= 2 => {
            export(&r, rest.get(1).map(String::as_str), a.has("json"))
        }
        Some("apply") if rest.len() == 2 => apply(&r, &rest[1], a.has("dry-run")),
        Some("cutover") if rest.len() == 2 => cutover(&r, &rest[1]),
        Some(v @ ("park" | "unpark" | "done")) if rest.len() == 2 => state(&r, v, &rest[1]),
        Some("repo") if rest.len() == 3 => code_repo(&r, &rest[1], &rest[2]),
        _ => Err(
            "rejected: fael plan import | fael plan next | fael plan export [<plan>] [--json | --section <key>] | fael plan apply <file|-> [--dry-run] | fael plan cutover <plan> | fael plan new <name> \"<title>\" | fael plan park|unpark|done <plan> | fael plan repo <plan> <path|-> — try 'fael plan --help'"
                .into(),
        ),
    }
    .map(|()| ExitCode::SUCCESS)
}

/// Beside the journal, so every worktree of the clone reads one db; the tree's `.fael/`
/// (or `FAEL_DIR`) without git.
pub(crate) fn db(r: &Repo) -> PathBuf {
    r.journal.as_deref().unwrap_or(&r.fael).join("plans.db")
}

fn import(r: &Repo) -> Result<(), String> {
    let plans = scan(&r.root);
    if plans.is_empty() {
        return Err(format!(
            "rejected: no PLAN-*.md under a .fapony/ in {} — nothing to import",
            r.root.display()
        ));
    }
    let path = db(r);
    let rep = Store::open(&path)?.import_all(&plans, &fael_core::rfc3339(fael_core::now_ms()))?;
    crate::board::register(r);
    println!(
        "imported {} plans, {} chunks → {}",
        rep.plans,
        rep.chunks,
        path.display()
    );
    if rep.drafts > 0 {
        println!(
            "⚠ {} unknown checkbox line(s) imported as draft (not startable) — use [ ], [x] or [~]",
            rep.drafts
        );
    }
    for u in &rep.unresolved {
        println!("⚠ (after …) names no chunk, never met: {u}");
    }
    for a in &rep.ambiguous {
        println!("⚠ label/title matches more than one chunk, new uid: {a}");
    }
    if rep.db_plans > 0 {
        println!(
            "{} plan(s) live in the db: plan fields refreshed, chunks untouched",
            rep.db_plans
        );
    }
    Ok(())
}

/// SPEC §9: re-import, so the db holds the md's chunks as written, then hand the chunk list
/// to the db and swap the md's chunk lines for the banner. The db flips first: a failed
/// write leaves the old lines, which no import reads any more.
fn cutover(r: &Repo, want: &str) -> Result<(), String> {
    // plans.db keeps one row per app/name: two files sharing it collapse on import, so
    // refuse before import rather than cut over whichever came last (issue 01M4MJ48)
    let plans = scan(&r.root);
    let mut hits: Vec<&Import> = plans
        .iter()
        .filter(|p| p.md.name == want || format!("{}/{}", p.app, p.md.name) == want)
        .collect();
    hits.sort_by(|a, b| (&a.app, &a.source).cmp(&(&b.app, &b.source)));
    if hits.windows(2).any(|w| w[0].app == w[1].app) {
        let paths: Vec<&str> = hits.iter().map(|p| p.source.as_str()).collect();
        return Err(format!(
            "rejected: '{want}' names more than one plan file: {} — rename one, or move the shipped one out of .fapony/",
            paths.join(", ")
        ));
    }
    import(r)?;
    let mut s = open(r)?;
    let p = one(&s, want)?;
    let n = s.cut_over(p.id)?;
    mirror(r, &s)?;
    println!(
        "{} cut over: its {n} chunks live in plans.db (`fael chunk …`); {} keeps Goal, Scope, Done and the rest",
        key(&p),
        p.source
    );
    Ok(())
}

/// `new`: the plan row (chunks in the db from the start) and its Goal/Scope/Done/Constraints
/// skeleton sections, no md file; `fael plan apply` fills them.
fn create(a: &Args, r: &Repo, name: &str, title: &str) -> Result<(), String> {
    let app = a.one("app").unwrap_or_default();
    let area = a.one("area").unwrap_or_else(|| "dev".into());
    let now = fael_core::rfc3339(fael_core::now_ms());
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
            mirror(r, &s)?;
            println!("applied (rev {rev})");
        }
    }
    Ok(())
}

/// `park` · `unpark` · `done` on a cut-over plan: the owner's word on `plan.state`.
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

/// `repo <plan> <path|->`: the checkout the plan's code lives in (its top level), `-` = this
/// repo again. Briefs name it; the board lists it as a project with its remote.
fn code_repo(r: &Repo, want: &str, path: &str) -> Result<(), String> {
    let mut s = open(r)?;
    let p = one(&s, want)?;
    let top = (path != "-")
        .then(|| {
            let dir = r.cwd.join(path);
            let top = crate::git(&dir, &["rev-parse", "--show-toplevel"])
                .ok_or(format!("rejected: {path} is not a git checkout"))?;
            Path::new(&top)
                .canonicalize()
                .map(|t| t.to_string_lossy().into_owned())
                .map_err(|e| format!("{top}: {e}"))
        })
        .transpose()?;
    s.set_repo(p.id, top.as_deref())?;
    crate::board::register(r);
    match top {
        Some(t) => println!("plan {} → code in {t}", key(&p)),
        None => println!("plan {} → code in this repo", key(&p)),
    }
    Ok(())
}

/// Each cut-over plan's md: its TL;DR lists the db's chunks again, read-only, so the owner
/// sees state in the md until the board app shows it. Import never reads them back.
pub(crate) fn mirror(r: &Repo, s: &Store) -> Result<(), String> {
    for p in s
        .plans()?
        .iter()
        .filter(|p| p.truth == "db" && !p.source.is_empty())
    {
        let file = r.root.join(&p.source);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let md = fael_core::plan::mirror(&text, &key(p), &s.ticks(p.id)?);
        if let Some(md) = md.filter(|md| *md != text) {
            std::fs::write(&file, md).map_err(|e| format!("{}: {e}", file.display()))?;
        }
    }
    Ok(())
}

/// `<root>/.fapony`, `<root>/*/.fapony`, `<root>/*/*/.fapony` (monorepo apps); each one's
/// `plan/`, `parked/`, `done/`. ponytail: two levels, no config — add a `plan_roots` key
/// when a repo nests apps deeper.
fn scan(root: &Path) -> Vec<Import> {
    let mut apps = vec![PathBuf::new()];
    for d in subdirs(root) {
        let rel = PathBuf::from(d.file_name().unwrap_or_default());
        apps.extend(subdirs(&d).map(|e| rel.join(e.file_name().unwrap_or_default())));
        apps.push(rel);
    }
    apps.sort();
    let mut out = Vec::new();
    for app in apps {
        let base = root.join(&app).join(".fapony");
        for dir in ["plan", "parked", "done"] {
            let Ok(rd) = std::fs::read_dir(base.join(dir)) else {
                continue;
            };
            let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            files.sort();
            for f in files {
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                let Some(parsed) = std::fs::read_to_string(&f)
                    .ok()
                    .and_then(|t| md::parse(name, &t))
                else {
                    continue;
                };
                out.push(Import {
                    app: app.to_string_lossy().replace('\\', "/"),
                    dir: dir.to_string(),
                    source: f
                        .strip_prefix(root)
                        .unwrap_or(&f)
                        .to_string_lossy()
                        .replace('\\', "/"),
                    md: parsed,
                });
            }
        }
    }
    out
}

fn subdirs(d: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            !n.starts_with('.') && !matches!(n.as_ref(), "node_modules" | "target")
        })
        .map(|e| e.path())
        .filter(|p| p.is_dir())
}

fn open(r: &Repo) -> Result<Store, String> {
    let path = db(r);
    if !path.exists() {
        return Err("rejected: no plans.db yet — run `fael plan import` first".into());
    }
    Store::open(&path)
}

/// `app/name`, or `name` at the root — how `next` and `export` name a plan.
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
            "rejected: no plan '{w}' in plans.db — see `fael plan next`"
        )),
        (Some(w), n) if n > 1 => Err(format!("rejected: '{w}' names {n} plans — use app/name")),
        _ => Ok(plans),
    }
}

fn one(s: &Store, want: &str) -> Result<PlanRow, String> {
    named(s, Some(want)).map(|mut v| v.remove(0))
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
    let md = |src: &str| std::fs::read_to_string(r.root.join(src)).ok();
    let docs: Vec<String> = plans
        .iter()
        .map(|p| fael_core::plan::export(&s, p.id, &md))
        .collect::<Result<_, _>>()?;
    print!("{}", docs.join("\n"));
    Ok(())
}

/// `export <plan> --section <key>`: one section of one plan.
fn section(r: &Repo, want: &str, key: &str) -> Result<(), String> {
    let s = open(r)?;
    print!(
        "{}",
        fael_core::plan::export_section(&s, one(&s, want)?.id, key)?
    );
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

fn next(r: &Repo) -> Result<(), String> {
    let s = open(r)?;
    let branch = crate::journal::head_branch(&r.root);
    // fapony `liveBranches`: a claim naming no checked-out branch holds nothing
    let live: Option<HashSet<String>> = crate::git(&r.root, &["worktree", "list", "--porcelain"])
        .map(|o| {
            o.lines()
                .filter_map(|l| l.strip_prefix("branch refs/heads/"))
                .map(str::to_string)
                .collect()
        });
    for p in s.plans()? {
        let what = match fael_core::plan::next(&s, p.id, branch.as_deref(), live.as_ref())? {
            Next::Chunk { text, .. } => format!("- [ ] {text}"),
            Next::NoneReady => "(none ready)".into(),
            Next::Blocked => "(blocked)".into(),
            Next::Parked => "(parked)".into(),
            Next::Closed => "(all closed)".into(),
            Next::Shipped => continue,
        };
        println!("{}\t{what}", key(&p));
    }
    Ok(())
}
