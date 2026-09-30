//! `fael purge <id>` — delete a leaked test row or a mistake for good.
//! Thin adapter: the repo is resolved here, the refusal rules and the rewrite
//! live in `fael-core` so a hosted server calls the same entry point.

use crate::{Args, core, hook};
use std::path::Path;

/// `fael purge <id>`: an exact id or unique prefix names the row; the row and
/// its close events go from every month file, tree and journal. Refusals come
/// from core (`supersedes`/`restores` edges, close-event ids, immutable files,
/// unreadable lines) and print as errors. The id is also kept as a tombstone
/// (`sync::purged`): the next `fael sync` carries it in this writer's ref so the
/// row does not come back from a remote; a set `fael.remote` adds a warning
/// that copies already in a teammate's journal stay until purged there too.
pub(crate) fn purge(r: &crate::Repo, a: &Args, id: &str) -> Result<(), String> {
    let log = crate::read(r);
    let out = core::purge_row(&r.fael, r.journal.as_deref(), &log, id)?;
    crate::sync::record_purge(r, &out.id)?;
    let mut warns = vec![];
    if synced_elsewhere(r) {
        warns.push(
            "warning: fael.remote is set — the next fael sync keeps this row from coming \
             back, but copies already in a teammate's journal stay until purged there too"
                .into(),
        );
    }
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "purge", Some(&r.root), &warns);
    let files = out
        .files
        .iter()
        .map(|p| display(r, p))
        .collect::<Vec<_>>()
        .join(", ");
    if a.has("json") {
        println!(
            "{}",
            serde_json::json!({
                "purged": out.id, "title": out.title,
                "rows": out.rows, "closes": out.closes, "files": files,
            })
        );
    } else {
        println!(
            "purged {} — {} ({} row(s), {} close(s) from {files})",
            out.id, out.title, out.rows, out.closes,
        );
    }
    Ok(())
}

/// Tree paths read repo-relative; journal paths (outside the repo) keep a
/// `journal/` prefix so the two stores never look like one file.
fn display(r: &crate::Repo, p: &Path) -> String {
    if let Ok(rel) = p.strip_prefix(&r.fael) {
        return rel.display().to_string();
    }
    if let Some(j) = r.journal.as_deref()
        && let Ok(rel) = p.strip_prefix(j)
    {
        return format!("journal/{}", rel.display());
    }
    p.display().to_string()
}

/// Whether this clone syncs anywhere — only then do teammates hold copies. The adapter side may spawn; core stays spawn-free.
fn synced_elsewhere(r: &crate::Repo) -> bool {
    std::process::Command::new("git")
        .args(["config", "--get", "fael.remote"])
        .current_dir(&r.root)
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}
