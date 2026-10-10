//! `fael mv <old> <new>` — moved out of main.rs at the 400-line cap.

use crate::{Args, core, hook, read, repo, stamp};

/// Record that `old` moved to `new` — for what git can't see (anchors,
/// uncommitted rewrites, repos without git). Appends an alias row; the log
/// stays append-only, nothing is rewritten.
pub(crate) fn mv(a: &Args, old: &str, new: &str) -> Result<(), String> {
    a.only("mv", &["json"])?;
    let r = repo()?;
    let norm = core::normalize_files(&[old.to_string(), new.to_string()], &r.cwd, &r.root)?;
    let (from, to) = (&norm[0], &norm[1]);
    if from == to {
        return Err(format!(
            "rejected: {from:?} is already itself — `fael mv` needs two different paths"
        ));
    }
    let log = read(&r);
    if core::Aliases::from_log(&log).forward(from).contains(to) {
        return Err(format!(
            "rejected: {from} → {to} is already recorded — `fael find --files {to}` shows the rows"
        ));
    }
    let (row, _, warns) =
        core::mv_row(&r.fael, r.journal.as_deref(), &r.cfg, &stamp(&r), from, to)?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "mv", Some(&r.root), &warns);
    if a.has("json") {
        println!("{}", row.to_line());
    } else {
        println!("{} → {from} → {to}", row.id);
    }
    Ok(())
}
