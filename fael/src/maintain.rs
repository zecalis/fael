//! `fael doctor` · `fael compact` · `fael import` — the maintenance commands
//! (SPEC §6, §11). Thin adapters: the repo is resolved here, the rules live
//! in `fael-core` so a hosted server calls the same entry points.

mod merged;
mod orphan;

use crate::{Args, core, repo};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn doctor(a: &Args) -> Result<ExitCode, String> {
    let r = repo()?;
    let month = core::current_month();
    let source = crate::hook::ignore_source(&r.root);
    let excluded = source.as_deref().is_some_and(crate::hook::deliberate);
    let ignored = source.is_some() && !excluded;
    if a.has("fix") {
        let before = core::doctor_scan(&r.fael, &r.root, ignored, &month);
        for action in core::doctor_fix(&r.fael, &r.root, &before)? {
            println!("fixed: {action}");
        }
    }
    let mut rep = core::doctor_scan(&r.fael, &r.root, ignored, &month);
    if excluded {
        rep.problems.push(core::Problem {
            kind: core::ProblemKind::Ignored,
            severity: core::Severity::Info,
            fixable: false,
            file: None,
            detail: ".fael/log is kept local by .git/info/exclude — taken as deliberate; \
                     move the pattern to .gitignore if it is not"
                .into(),
        });
    }
    let log = crate::read(&r);
    // Gone is judged through the resolver: a file that was renamed still
    // exists under its new path, so its rows still push and are not gone.
    let al = crate::aliases::load(&r, &log, true);
    rep.problems.extend(open_row_notes(&log, &r.root, &al));
    show(&rep, a.has("json"));
    Ok(if rep.errors().count() > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// The open-row checks (Gone/PartGone/Stale/Orphan): what the rows still say
/// versus what the repo (and its PRs) still hold. Split out of `doctor` for
/// the 100-line rule — the `gh` half of Orphan lives here, never in core.
fn open_row_notes(log: &core::Log, root: &Path, al: &core::Aliases) -> Vec<core::Problem> {
    let mut out = vec![];
    let gone: Vec<_> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter(|row| core::gone(root, row, al))
        .collect();
    if !gone.is_empty() {
        let w = core::abbrev(log);
        let eg: Vec<String> = gone
            .iter()
            .take(5)
            .map(|row| {
                format!(
                    "{} → {}",
                    &row.id[..w.min(row.id.len())],
                    row.files.join(", ")
                )
            })
            .collect();
        out.push(core::Problem {
            kind: core::ProblemKind::Gone,
            severity: core::Severity::Info,
            fixable: false,
            file: None,
            detail: format!(
                "{} open row(s) name only files that no longer exist, so they never push — \
                 re-file them on the new path or `fael close` them (e.g. {})",
                gone.len(),
                eg.join("; ")
            ),
        });
    }
    // some files gone, some left: the row still pushes, but it likely describes
    // the repo as it was (a tool swapped out, a config file removed)
    let part: Vec<String> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter(|row| !core::gone(root, row, al))
        .filter_map(|row| {
            let g = core::gone_files(root, row, al);
            let w = core::abbrev(log);
            (!g.is_empty())
                .then(|| format!("{} → {}", &row.id[..w.min(row.id.len())], g.join(", ")))
        })
        .collect();
    if !part.is_empty() {
        out.push(core::Problem {
            kind: core::ProblemKind::PartGone,
            severity: core::Severity::Info,
            fixable: false,
            file: None,
            detail: format!(
                "{} open row(s) still name a file that no longer exists — check the text still \
                 holds, then re-file with `--supersedes` or `fael close` (e.g. {})",
                part.len(),
                part[..part.len().min(5)].join("; ")
            ),
        });
    }
    // prose rot: the row's text points at a backticked path with no file
    // behind it, so the next reader follows a dead pointer
    let stale = stale_rows(log, root, al);
    if !stale.is_empty() {
        out.push(core::Problem {
            kind: core::ProblemKind::Stale,
            severity: core::Severity::Info,
            fixable: false,
            file: None,
            detail: format!(
                "{} open row(s) name a path in backticks that is not on disk — check the text \
                 still holds, then re-file with `--supersedes` or `fael close` (e.g. {})",
                stale.len(),
                stale[..stale.len().min(5)].join("; ")
            ),
        });
    }
    // orphaned branches: open rows filed where the PR died unmerged, so the
    // next reader keeps following work that will never land
    let orphan = orphan::rows(log);
    if !orphan.is_empty() {
        let n: usize = orphan.iter().map(|(_, ids)| ids.len()).sum();
        let eg: Vec<String> = orphan
            .iter()
            .take(5)
            .map(|(b, ids)| format!("{b} → {}", ids.join(", ")))
            .collect();
        out.push(core::Problem {
            kind: core::ProblemKind::Orphan,
            severity: core::Severity::Info,
            fixable: false,
            file: None,
            detail: format!(
                "{n} open row(s) filed on branch(es) whose PR was closed without merge — \
                 the work likely died with the branch; re-file with `--supersedes` or `fael close` (e.g. {})",
                eg.join("; ")
            ),
        });
    }
    // landed branches: merged upstream but still sitting in this clone, so
    // the next reader keeps wondering whether the work is done
    out.extend(merged::problem(root));
    out
}

/// `short-id → dead backticked path(s)` for every open row whose text still
/// points at a path with no file behind it (row-hygiene chunk 4).
fn stale_rows(log: &core::Log, root: &Path, al: &core::Aliases) -> Vec<String> {
    core::find(log, &core::Filter::default())
        .into_iter()
        .filter_map(|row| {
            let refs = core::stale_refs(root, row, al);
            let w = core::abbrev(log);
            (!refs.is_empty())
                .then(|| format!("{} → {}", &row.id[..w.min(row.id.len())], refs.join(", ")))
        })
        .collect()
}

fn show(rep: &core::DoctorReport, json: bool) {
    if json {
        let ps: Vec<_> = rep
            .problems
            .iter()
            .map(|p| {
                serde_json::json!({
                    "kind": format!("{:?}", p.kind).to_lowercase(),
                    "severity": format!("{:?}", p.severity).to_lowercase(),
                    "fixable": p.fixable,
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
        println!("{sev} [{:?}]{fix}: {}", p.kind, p.detail);
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
    let rep = core::compact(&r.fael, &r.root, &opts, &core::current_month(), &al)?;
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
    let ok = b.len() == 7
        && b.as_bytes()[4] == b'-'
        && b.bytes()
            .enumerate()
            .all(|(i, c)| i == 4 || c.is_ascii_digit());
    if ok {
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
    let rep = core::import(&r.fael, &sp, &r.cfg.kinds, &core::ImportOpts { maps })?;
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
