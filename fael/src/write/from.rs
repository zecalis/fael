//! `from: user` — whose call a row records. Only `user` is a value; absent
//! means the agent chose, and a later agent may revise its own call but asks
//! the user before changing theirs.

use crate::core;

/// `from`: only `user` is a value — the agent's own choice is the default.
pub(super) fn from_user(from: Option<String>) -> Result<Option<String>, String> {
    match from
        .map(|f| f.trim().to_lowercase())
        .filter(|f| !f.is_empty())
    {
        Some(f) if f != "user" => Err(format!(
            "rejected: from {f:?} — the only value is `user` (the user said or decided it); omit it when you chose"
        )),
        f => Ok(f),
    }
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
