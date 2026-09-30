//! `fael migrate local` — move a tracked repo to `store = "local"` without
//! losing rows. Thin adapter: the fold lives in `fael-core` (`adopt_tree`);
//! this resolves the repo, flips the config line and says what is left.

use crate::core;
use std::path::Path;

pub(crate) fn local(r: &crate::Repo) -> Result<(), String> {
    let j = r.journal.as_deref().ok_or(
        "rejected: no git repo here — the journal lives in the git dir, so there is no local store to move to",
    )?;
    let out = core::adopt_tree(&r.fael, j)?;
    set_store(&r.fael.join("config.toml"))?;
    println!(
        "folded the tree log into {}: {} line(s) copied, {} stale journal line(s) replaced ({} file(s))",
        j.display(),
        out.copied,
        out.replaced,
        out.files.len()
    );
    println!("store = \"local\" in .fael/config.toml — commit it; new rows stay out of the tree");
    if r.fael.join("log").is_dir() {
        println!(
            "the tree log is frozen history now and still read: keep it, or remove it \
             (git rm -r --cached .fael/log) once every clone has run `fael migrate local`"
        );
    }
    Ok(())
}

/// `store = "local"` as a top-level key: an existing `store =` line is
/// replaced, else the line goes first (before any `[table]`).
fn set_store(path: &Path) -> Result<(), String> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let (mut found, mut table) = (false, false);
    let mut lines: Vec<String> = old
        .lines()
        .map(|l| {
            table |= l.trim_start().starts_with('[');
            let key = l.split_once('=').map(|(k, _)| k.trim());
            if !found && !table && key == Some("store") {
                found = true;
                "store = \"local\"".to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        lines.insert(0, "store = \"local\"".into());
    }
    let body = lines.join("\n") + "\n";
    // the file must still parse — a broken config fails every command
    core::Config::from_toml(&body).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))
}
