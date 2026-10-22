//! `from` — whose call a row records. `user` is the user's call; `report` is
//! a claim relayed from someone else's report, not checked against the code;
//! absent means the agent chose. A later agent may revise its own call but
//! asks the user before changing theirs.

use crate::core;

/// `from`: `user` or `report` — the agent's own choice is the default.
pub(super) fn from_user(from: Option<String>) -> Result<Option<String>, String> {
    match from
        .map(|f| f.trim().to_lowercase())
        .filter(|f| !f.is_empty())
    {
        Some(f) if f != "user" && f != "report" => Err(format!(
            "rejected: from {f:?} — the values are `user` (the user said or decided it) and `report` (relayed from someone else's report, not checked against the code); omit it when you chose"
        )),
        f => Ok(f),
    }
}

/// A new issue names the open decisions and notes already on its files (at most 2, anchors
/// skipped) — surfacing only, so the filer sees what the code's neighbours already say.
pub(super) fn on_files(log: &core::Log, row: &core::Row, sup: Option<&str>) -> Vec<String> {
    if row.kind != "issue" {
        return vec![];
    }
    let gone: std::collections::HashSet<&str> = core::closed(log)
        .union(&core::superseded(log))
        .copied()
        .collect();
    let ab = core::abbrev(log);
    log.rows
        .iter()
        .filter(|o| matches!(o.kind.as_str(), "decision" | "note"))
        .filter(|o| !gone.contains(o.id.as_str()) && Some(o.id.as_str()) != sup)
        .filter(|o| {
            o.files
                .iter()
                .any(|f| core::anchor(f).is_none() && row.files.contains(f))
        })
        .take(2)
        .map(|o| {
            format!(
                "on these files: {} \"{}\"",
                ab.short(&o.id),
                o.display_title()
            )
        })
        .collect()
}

/// A row that replaces the user's call without being the user's says so:
/// the user decided the old one, so they are the one to change it.
pub(super) fn over_user(log: &core::Log, row: &core::Row, sup: Option<&str>) -> Option<String> {
    let old = log.rows.iter().find(|o| Some(o.id.as_str()) == sup)?;
    (old.from_user() && !row.from_user()).then(|| {
        format!(
            "warning: this supersedes {}, the user's call — confirm with the user first, or set from user if they said it",
            core::abbrev(log).short(&old.id)
        )
    })
}
