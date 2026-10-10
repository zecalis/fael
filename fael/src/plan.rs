//! `fael plan import` · `fael plan next` (PLAN-fael-board b1): read every `.fapony/` of the
//! repo into `plans.db`, and print each plan's next chunk from the db — the same pick
//! `fapony plan` makes, so `scripts/plan-parity.sh` can hold the two to each other.

use crate::{Args, Repo, repo};
use fael_core::plan::{Import, Next, Store, md};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    a.only("plan", &[])?;
    let r = repo()?;
    match rest.first().map(String::as_str) {
        Some("import") if rest.len() == 1 => import(&r),
        Some("next") if rest.len() == 1 => next(&r),
        _ => Err("rejected: fael plan import | fael plan next — try 'fael plan --help'".into()),
    }
    .map(|()| ExitCode::SUCCESS)
}

/// Beside the journal, so every worktree of the clone reads one db; the tree's `.fael/`
/// (or `FAEL_DIR`) without git.
fn db(r: &Repo) -> PathBuf {
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

fn next(r: &Repo) -> Result<(), String> {
    let path = db(r);
    if !path.exists() {
        return Err("rejected: no plans.db yet — run `fael plan import` first".into());
    }
    let s = Store::open(&path)?;
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
        let key = if p.app.is_empty() {
            p.name
        } else {
            format!("{}/{}", p.app, p.name)
        };
        println!("{key}\t{what}");
    }
    Ok(())
}
