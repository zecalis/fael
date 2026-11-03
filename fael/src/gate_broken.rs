//! `fael gate-broken --check <name> --files <f> [--on <sha>] [--detail "..."]`:
//! the gate failed, and the same failure reproduces on main — file it once as
//! an issue keyed `gate:<check>` so the next agent finds it instead of
//! re-proving a broken main. Dedup: an open row on the key reports its id and
//! writes nothing (one open row per check, like intake's per-ticket row).

use std::process::ExitCode;

use crate::core::Filter;

/// CLI: `fael gate-broken --check <name> --files <f> [--on <sha>] [--detail "..."] [--json]`.
pub(crate) fn cmd(a: &crate::Args) -> Result<ExitCode, String> {
    a.only("gate-broken", &["check", "files", "on", "detail", "json"])?;
    let check = a
        .one("check")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or("rejected: --check needs a gate name — e.g. --check clippy".to_string())?;
    let key = format!("gate:{check}");
    crate::core::valid_key(&key)?;
    let files = a.files();
    if files.is_empty() {
        return Err("rejected: --files is required — name the file the check failed on".into());
    }
    let r = crate::repo()?;
    let sha = match a
        .one("on")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(s) => s,
        None => crate::git(&r.root, &["rev-parse", "origin/main"])
            .or_else(|| crate::git(&r.root, &["rev-parse", "HEAD"]))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or("rejected: --on needs a sha — no origin/main or HEAD here".to_string())?,
    };
    let log = crate::read(&r);
    if let Some(open) = crate::core::find(&log, &Filter::default())
        .into_iter()
        .find(|x| x.key.as_deref() == Some(key.as_str()))
    {
        println!(
            "{key} already has an open issue {} — nothing filed",
            open.id
        );
        return Ok(ExitCode::SUCCESS);
    }
    let detail = a
        .one("detail")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let mut text = format!(
        "main ({sha}) fails {check} on {}. Verify on a clean main checkout, then close with the fix.",
        files.join(", ")
    );
    if let Some(d) = detail {
        text = format!("{d}; {text}");
    }
    let (row, path, warns) = crate::write::add_row(
        &r,
        "issue",
        &text,
        &files,
        crate::write::AddOpts {
            key: Some(key),
            to: None,
            from: None,
            title: Some(format!("main is broken: {check}")),
            revisit: None,
            urgent: crate::core::Urgent::Unset,
            supersedes: None,
            follows: None,
            force: false,
            by: None,
        },
    )?;
    crate::batch::emit_warns(&warns);
    crate::hook::record_row_asks("cli", "add", &r.root, &row, &warns);
    crate::batch::written(a, &r, &row, &path);
    Ok(ExitCode::SUCCESS)
}
