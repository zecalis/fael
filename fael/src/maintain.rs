//! `fael doctor` · `fael compact` · `fael import` — the maintenance commands
//! (SPEC §6, §11). Thin adapters: the repo is resolved here, the rules live
//! in `fael-core` so a hosted server calls the same entry points.

mod alive;
mod drift;
mod fat;
mod merged;
mod noverdict;
mod orphan;
mod phantom;
mod rows;
mod shipped;
mod status;
mod unstamped;

use crate::{Args, core, repo};
pub(crate) use alive::BranchFiles;

/// Full ids of `rows` whose every file is gone from `root` — through the
/// alias resolver, and never a file alive on the row's own unmerged branch
/// (same judgement as `doctor [Gone]`). Chunk 4's tag for the grouped issue
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
    // same symlink note as `fael install` (stdout only — `--json` stays pure JSON)
    if !a.has("json")
        && matches!(r.cfg.store, core::Store::Tracked)
        && std::fs::symlink_metadata(&r.fael).is_ok_and(|m| m.file_type().is_symlink())
    {
        println!(
            "note: {} is a symlink — store = \"local\" in .fael/config.toml keeps rows in this clone instead",
            r.fael.display()
        );
    }
    let month = core::current_month();
    let local = matches!(r.cfg.store, core::Store::Local);
    // `local` keeps the log out of git on purpose: no ignore check to run
    let source = (!local)
        .then(|| crate::hook::ignore_source(&r.root))
        .flatten();
    let excluded = source.as_deref().is_some_and(crate::hook::deliberate);
    let ignored = source.is_some() && !excluded;
    // Rows may live only in the clone's journal (`store = "local"`, a fresh
    // worktree): scan them there. Nothing of it is in git, so the `merge=union`
    // and gitignore checks do not apply — nor under `local`, where a tree log
    // is frozen history nothing appends to any more.
    let home = crate::journal::home(&r).unwrap_or(&r.fael);
    let scan = || {
        let mut rep = core::doctor_scan(home, &r.root, ignored, &month);
        if home != r.fael || local {
            rep.problems.retain(|p| {
                !matches!(
                    p.kind,
                    core::ProblemKind::Union | core::ProblemKind::Ignored
                )
            });
        }
        rep
    };
    if a.has("fix") {
        for action in core::doctor_fix(home, &r.root, &scan())? {
            say(a.has("json"), &format!("fixed: {action}"));
        }
    }
    let mut rep = scan();
    if local {
        rep.problems.push(core::Problem::info(
            core::ProblemKind::Local,
            local_note(&r),
        ));
    }
    if excluded {
        rep.problems.push(core::Problem::info(
            core::ProblemKind::Ignored,
            ".fael/log is kept local by .git/info/exclude — taken as deliberate; \
             move the pattern to .gitignore if it is not"
                .into(),
        ));
    }
    let log = crate::read(&r);
    if let Some(l) = crate::sync::late_line(&r, &log) {
        let l = l.strip_prefix("fael: ").unwrap_or(&l).to_string();
        rep.problems
            .push(core::Problem::info(core::ProblemKind::Late, l));
    }
    let behind = crate::install::pending();
    if behind > 0 {
        rep.problems.push(core::Problem::info(
            core::ProblemKind::Wiring,
            format!(
                "{behind} client wiring change(s) pending (hooks, plugin or skill behind this \
                 binary) — `fael upgrade` applies them; until then a newer hook stays off \
                 (note: the prompt hint is Claude-only — Codex has no UserPromptSubmit hook)"
            ),
        ));
    }
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
    // `--fix` also closes the confirmed `[Shipped]` notes — the adapter half
    // (core never runs `gh`, so it can never take this action itself).
    if a.has("fix") {
        fix_shipped(&r, &mut rep, a.has("json"));
    }
    show(&rep, a.has("json"));
    Ok(if rep.errors().count() > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Where a `local` repo's rows live, and — with no `fael.remote` — that
/// nothing carries them off this clone yet.
fn local_note(r: &crate::Repo) -> String {
    let at = r
        .journal
        .as_deref()
        .map_or_else(|| r.fael.display().to_string(), |j| j.display().to_string());
    let mut s =
        format!("store = local — rows live in {at}, shared by every worktree of this clone");
    if crate::git(&r.root, &["config", "fael.remote"]).is_none() {
        s.push_str(
            "; nothing copies them off this clone — `git config fael.remote <url>` then `fael sync` to back up or share",
        );
    }
    s
}

/// The adapter half of `doctor --fix`: close the confirmed `[Shipped]` notes,
/// then drop them from the report so a second `doctor` reads clean. `[Shipped?]`
/// never carries close actions (its merge time is unknown), and a close that
/// fails (a concurrent close, a bump) leaves its row in place.
fn fix_shipped(r: &crate::Repo, rep: &mut core::DoctorReport, json: bool) {
    for p in rep
        .problems
        .iter_mut()
        .filter(|p| p.kind == core::ProblemKind::Shipped)
    {
        let mut left = vec![];
        for (id, text) in p.closes.drain(..) {
            match crate::write::close_row(r, &id, &text) {
                Ok((row, _, _)) => say(json, &format!("fixed: closed {} — {text}", row.id)),
                Err(e) => {
                    eprintln!("skip: {e}");
                    left.push((id, text));
                }
            }
        }
        p.closes = left;
        p.ids = p.closes.iter().map(|(id, _)| id.clone()).collect();
    }
    rep.problems
        .retain(|p| p.kind != core::ProblemKind::Shipped || !p.closes.is_empty());
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
    let rep = core::import(
        &r.fael,
        r.journal.as_deref(),
        r.cfg.store,
        &sp,
        &r.cfg.kinds,
        &core::ImportOpts { maps },
    )?;
    // imported rows are older than the late watermark but pushed by this clone
    if rep.adds > 0 {
        crate::sync::forget_mark(&r);
    }
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
