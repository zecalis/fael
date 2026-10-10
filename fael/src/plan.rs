//! `fael plan import` · `next` · `export` · `cutover` (PLAN-fael-board b1, b1b, c1): read
//! every `.fapony/` of the repo into `plans.db`, print each plan's next chunk from the db —
//! the same pick `fapony plan` makes, so `scripts/plan-parity.py` can hold the two to each
//! other — write a plan back out as markdown, and hand one plan's chunks to the db.

use crate::{Args, Repo, repo};
use fael_core::plan::{Import, Next, PlanRow, Store, md};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    a.only("plan", &[])?;
    let r = repo()?;
    match rest.first().map(String::as_str) {
        Some("import") if rest.len() == 1 => import(&r),
        Some("next") if rest.len() == 1 => next(&r),
        Some("export") if rest.len() <= 2 => export(&r, rest.get(1).map(String::as_str)),
        Some("cutover") if rest.len() == 2 => cutover(&r, &rest[1]),
        _ => Err(
            "rejected: fael plan import | fael plan next | fael plan export [<plan>] | fael plan cutover <plan> — try 'fael plan --help'"
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

/// Every plan, or the one `want` names (`name` or `app/name`), to stdout.
fn export(r: &Repo, want: Option<&str>) -> Result<(), String> {
    let s = open(r)?;
    let plans = named(&s, want)?;
    let docs: Vec<String> = plans
        .iter()
        .map(|p| fael_core::plan::export(&s, p.id))
        .collect::<Result<_, _>>()?;
    print!("{}", docs.join("\n"));
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
