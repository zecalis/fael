//! Session-start vault sync (chunk session): ingest human inbox notes, then
//! reconcile dormancy — fail-open, silent unless something broke. No vault
//! dir = one `is_dir` check, zero cost; the hook says nothing new about it.

use crate::Repo;

/// Ingest `inbox/*.md` when any exist, then `reconcile` so a row gone
/// dormant loses its file. Returns `(warnings, changed)`: warnings ride the
/// hook's existing notice lines, `changed` tells session-start to re-read
/// the log the ingest just wrote to.
pub(crate) fn session_sync(r: &Repo) -> (Vec<String>, bool) {
    let Some((vault, _)) = super::vault_dir(r) else {
        return (vec![], false);
    };
    if !vault.is_dir() {
        return (vec![], false);
    }
    let mut warns = vec![];
    let mut changed = false;
    let inbox = vault.join("inbox");
    let mut names: Vec<String> = vec![];
    if let Ok(rd) = std::fs::read_dir(&inbox) {
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x == "md")
                && let Some(n) = e.file_name().to_str().map(String::from)
            {
                names.push(n);
            }
        }
    }
    names.sort();
    // never create the inbox here: a missing dir means nobody drops notes,
    // and the hook leaves no side effects behind.
    for n in &names {
        match super::inbox::file(r, &inbox.join(n)) {
            Ok(_) => changed = true,
            Err(e) => warns.push(format!("vault: inbox/{n}: {e}")),
        }
    }
    if let Err(e) = super::reconcile(r) {
        warns.push(format!("vault: {e}"));
    }
    (warns, changed)
}
