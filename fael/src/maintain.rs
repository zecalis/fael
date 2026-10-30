//! `fael doctor` · `fael compact` · `fael import` — the maintenance commands
//! (SPEC §6, §11). Thin adapters: the repo is resolved here, the rules live
//! in `fael-core` so a hosted server calls the same entry points.

mod alive;
pub(crate) use fael_core::drift;
mod fat;
mod merged;
mod noverdict;
mod orphan;
mod phantom;
mod rows;
mod shared;
mod shipped;
mod status;
mod unstamped;

use crate::{Args, core, repo};
pub(crate) use alive::BranchFiles;
pub(crate) use merged::{default_branch, head_branch};

/// Full ids of `rows` whose every file is gone from `root` — through the
/// alias resolver, and never a file alive on the row's own unmerged branch
/// (same judgement as `doctor [Gone]`). The `[Gone]` tag for the issue
/// list; partial losses stay untagged here (`doctor` reports `[PartGone]`).
pub(crate) fn gone_ids(
    root: &std::path::Path,
    al: &core::Aliases,
    rows: &[&core::Row],
) -> std::collections::HashSet<String> {
    let branches = BranchFiles::new(root);
    rows.iter()
        .filter(|r| !r.files.is_empty())
        .filter(|r| branches.missing(r, core::gone_files(root, r, al)).len() == r.files.len())
        .map(|r| r.id.clone())
        .collect()
}
use std::path::PathBuf;
use std::process::ExitCode;

pub fn doctor(a: &Args) -> Result<ExitCode, String> {
    let r = repo()?;
    let month = core::current_month();
    // rows live in the clone's journal (a fresh worktree has no tree log): scan
    // them there; a tree log is frozen history nothing appends to any more
    let home = crate::journal::home(&r).unwrap_or(&r.fael);
    let scan = || core::doctor_scan(home, &r.root, &month);
    if a.has("fix") {
        for action in core::doctor_fix(home, &scan())? {
            say(a.has("json"), &format!("fixed: {action}"));
        }
    }
    let mut rep = scan();
    rep.problems.push(core::Problem::info(
        core::ProblemKind::Local,
        local_note(&r),
    ));
    let log = crate::read(&r);
    let behind = crate::install::pending();
    if behind > 0 {
        rep.problems.push(core::Problem::info(
            core::ProblemKind::Wiring,
            format!(
                "{behind} client wiring change(s) pending (hooks, plugin or skill behind this \
                 binary) — `fael upgrade` applies them; until then a newer hook stays off"
            ),
        ));
    }
    rep.problems.extend(shared::notes(&r.root));
    // Gone is judged through the resolver: a file that was renamed still
    // exists under its new path, so its rows still push and are not gone.
    let al = crate::aliases::load(&r, &log, true);
    rep.problems.extend(rows::open_row_notes(
        &log,
        &r.root,
        &al,
        &r.cfg,
        a.has("fat"),
    ));
    // `--fix` also applies the mechanical closes the report carries
    // (confirmed `[Shipped]` notes, stuck `[Superseded]` chains, `[Gone]`
    // rows) — the adapter half (core never runs `gh`, so it can never take
    // this action itself). Anything else stays report-only with its ready
    // `fael close` command.
    if a.has("fix") {
        fix_closes(&r, &mut rep, a.has("json"));
    }
    show(&rep, a.has("json"));
    Ok(if rep.errors().count() > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Where a `local` repo's rows live, and which crew.db the board reads.
/// The 0-byte `.fael/crew.db` stub confuses (the real db is
/// `<git-common-dir>/fael/crew.db`): name the resolved path so the next
/// reader stops hunting.
fn local_note(r: &crate::Repo) -> String {
    let at = r
        .journal
        .as_deref()
        .map_or_else(|| r.fael.display().to_string(), |j| j.display().to_string());
    let dir = r.journal.as_deref().unwrap_or(&r.fael);
    let db = dir.join("crew.db");
    let old = dir.join("fael.db");
    let resolved = if db.exists() {
        db.display().to_string()
    } else if old.exists() {
        format!("{} (pre-rename fael.db)", old.display())
    } else {
        format!("{} (missing - run plan new to create it)", db.display())
    };
    let mut note = format!(
        "rows live in {at}, shared by every worktree of this clone; crew.db resolves to {resolved}"
    );
    if r.journal.is_some() {
        let stray = r.fael.join("crew.db");
        if stray.exists() {
            let n = std::fs::metadata(&stray).map(|m| m.len()).unwrap_or(0);
            note.push_str(&format!(
                "; stray {} ({} bytes) is not read while the journal lives — remove it",
                stray.display(),
                n
            ));
        }
    }
    note
}

/// The adapter half of `doctor --fix`: apply every mechanical close the report
/// carries (`[Shipped]` notes, stuck `[Superseded]` chains, `[Gone]` rows),
/// then drop the emptied problems so a second `doctor` reads clean. Only
/// problems with close actions are touched — every other kind is report-only
/// by construction (its `closes` are empty). `[Shipped?]` never carries close
/// actions (its merge time is unknown), and a close that fails (a concurrent
/// close, a bump) leaves its row in place. A row named by two tiers in one
/// pass (both Gone and Superseded-stuck) closes once: the second close would
/// be a no-op retry, so it is skipped without a second `fixed:` line.
fn fix_closes(r: &crate::Repo, rep: &mut core::DoctorReport, json: bool) {
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut emptied: Vec<usize> = vec![];
    for (i, p) in rep
        .problems
        .iter_mut()
        .enumerate()
        .filter(|(_, p)| !p.closes.is_empty())
    {
        let mut left = vec![];
        for (id, text) in p.closes.drain(..) {
            if !done.insert(id.clone()) {
                continue;
            }
            match crate::write::close_row(r, &id, &text, true) {
                Ok((row, _, _)) => say(json, &format!("fixed: closed {} — {text}", row.id)),
                Err(e) => {
                    eprintln!("skip: {e}");
                    left.push((id, text));
                }
            }
        }
        p.closes = left;
        p.ids = p.closes.iter().map(|(id, _)| id.clone()).collect();
        if p.closes.is_empty() {
            emptied.push(i);
        }
    }
    for i in emptied.into_iter().rev() {
        rep.problems.remove(i);
    }
}

/// A `--fix` action line. `--json` must stay pure JSON on stdout, so the
/// diagnostics move to stderr there (same for the core content fixes).
fn say(json: bool, line: &str) {
    if json {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

fn show(rep: &core::DoctorReport, json: bool) {
    if json {
        let ps: Vec<_> = rep
            .problems
            .iter()
            .map(|p| {
                serde_json::json!({
                    "kind": kind_label(&p.kind).to_lowercase(),
                    "severity": format!("{:?}", p.severity).to_lowercase(),
                    "fixable": p.fixable,
                    "ids": p.ids,
                    "detail": p.detail,
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(ps));
        return;
    }
    if rep.problems.is_empty() {
        println!("fael doctor: clean");
        return;
    }
    let errs = rep.errors().count();
    println!(
        "fael doctor: {} problem(s) ({} error(s), {} note(s))",
        rep.problems.len(),
        errs,
        rep.problems.len() - errs
    );
    for p in &rep.problems {
        let sev = if p.severity == core::Severity::Error {
            "error"
        } else {
            "note"
        };
        let fix = if p.fixable { " [--fix]" } else { "" };
        println!("{sev} [{}]{fix}: {}", kind_label(&p.kind), p.detail);
    }
}

/// The `[Shipped?]` / `[Shipped kept]` labels for the two Shipped variants —
/// every other kind renders as its debug name.
fn kind_label(k: &core::ProblemKind) -> String {
    match k {
        core::ProblemKind::ShippedMaybe => "Shipped?".into(),
        core::ProblemKind::ShippedKept => "Shipped kept".into(),
        _ => format!("{k:?}"),
    }
}

pub fn compact(a: &Args) -> Result<ExitCode, String> {
    let r = repo()?;
    if let Some(b) = a.one("before") {
        valid_month(&b)?;
    }
    let opts = core::CompactOpts {
        writer: a.one("writer"),
        before: a.one("before"),
        prune: a.has("prune"),
    };
    // prune judges "gone" through the resolver, so the aliases are loaded
    // the same way kickoff loads them (refresh = pick up latest renames)
    let al = crate::aliases::load(&r, &crate::read(&r), true);
    let rep = core::compact(
        &r.fael,
        r.journal.as_deref(),
        &r.root,
        &opts,
        &core::current_month(),
        &al,
    )?;
    // a pruned row is tombstoned like a purged one, so `fael sync` cannot bring
    // it back from this writer's own ref (docs/sync-format.md)
    for id in rep.writers.iter().flat_map(|w| &w.pruned_ids) {
        crate::sync::record_purge(&r, id)?;
    }
    if a.has("json") {
        let ws: Vec<_> = rep
            .writers
            .iter()
            .map(|w| {
                serde_json::json!({
                    "writer": w.writer, "rows": w.rows, "folded": w.folded,
                    "pruned": w.pruned, "carried": w.carried, "deleted": w.deleted,
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(ws));
    } else {
        for w in &rep.writers {
            println!(
                "{}: {} row(s) compacted ({} close(s) folded, {} pruned, {} carried) — deleted {}",
                w.writer,
                w.rows,
                w.folded,
                w.pruned,
                w.carried,
                w.deleted.join(", ")
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn valid_month(b: &str) -> Result<(), String> {
    if core::is_month(b) {
        Ok(())
    } else {
        Err(format!(
            "rejected: --before {b:?} is not yyyy-mm (e.g. 2026-08)"
        ))
    }
}

pub fn import(a: &Args, src: &str) -> Result<ExitCode, String> {
    let r = repo()?;
    let mut maps = vec![];
    for m in a.many("map") {
        let (old, new) = m
            .split_once('=')
            .filter(|(o, n)| !o.is_empty() && !n.is_empty())
            .ok_or_else(|| format!("rejected: --map {m:?} — write --map old/=new/"))?;
        maps.push((old.to_string(), new.to_string()));
    }
    let sp = PathBuf::from(src);
    let sp = if sp.is_absolute() { sp } else { r.cwd.join(sp) };
    // a repo root: its journal, where every row lands since #418, plus a frozen
    // tree log if one is left — never every *.jsonl under the root (fixtures)
    let journal = sp.join(".git").exists().then(|| crate::journal::root(&sp));
    let journal = journal
        .flatten()
        .map(|j| j.join("log"))
        .filter(|d| d.is_dir());
    let (sp, also) = match journal {
        Some(j) => {
            let tree = sp.join(".fael/log");
            (j, tree.is_dir().then_some(tree).into_iter().collect())
        }
        None => (sp, vec![]),
    };
    let rep = core::import(
        &r.fael,
        r.journal.as_deref(),
        &sp,
        &core::ImportOpts { maps, also },
    )?;
    for w in &rep.warnings {
        eprintln!("{w}");
    }
    if a.has("json") {
        println!(
            "{}",
            serde_json::json!({
                "adds": rep.adds, "folded": rep.folded, "carried": rep.carried,
                "skipped": rep.skipped,
                "paths": rep.paths.iter().map(|p| p.strip_prefix(&r.root).unwrap_or(p).display().to_string()).collect::<Vec<_>>(),
            })
        );
    } else {
        let paths = rep
            .paths
            .iter()
            .map(|p| p.strip_prefix(&r.root).unwrap_or(p).display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "imported {} row(s) ({} close(s) folded, {} carried, {} skipped) → {paths}",
            rep.adds, rep.folded, rep.carried, rep.skipped
        );
    }
    Ok(ExitCode::SUCCESS)
}
