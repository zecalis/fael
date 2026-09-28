//! `fael doctor` · `fael compact` · `fael import` — the maintenance commands
//! (SPEC §6, §11). Thin adapters: the repo is resolved here, the rules live
//! in `fael-core` so a hosted server calls the same entry points.

mod fat;
mod merged;
mod orphan;
mod rows;
mod shipped;

use crate::{Args, core, repo};
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
    let source = crate::hook::ignore_source(&r.root);
    let excluded = source.as_deref().is_some_and(crate::hook::deliberate);
    let ignored = source.is_some() && !excluded;
    if a.has("fix") {
        let before = core::doctor_scan(&r.fael, &r.root, ignored, &month);
        for action in core::doctor_fix(&r.fael, &r.root, &before)? {
            say(a.has("json"), &format!("fixed: {action}"));
        }
    }
    let mut rep = core::doctor_scan(&r.fael, &r.root, ignored, &month);
    if excluded {
        rep.problems.push(core::Problem::info(
            core::ProblemKind::Ignored,
            ".fael/log is kept local by .git/info/exclude — taken as deliberate; \
             move the pattern to .gitignore if it is not"
                .into(),
        ));
    }
    let log = crate::read(&r);
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
            match crate::close_row(r, &id, &text) {
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

/// The `[Shipped?]` label for the unconfirmed variant — every other kind
/// renders as its debug name.
fn kind_label(k: &core::ProblemKind) -> String {
    if *k == core::ProblemKind::ShippedMaybe {
        "Shipped?".into()
    } else {
        format!("{k:?}")
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
