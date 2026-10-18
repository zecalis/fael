//! `fael chunk list|ls`: the verb that never was one — the reads that list chunks.
//!
//! A bare `chunk` call with a wrong verb rejects with the whole usage line; `list`
//! and `ls` have an obvious intent, so they answer with the two reads instead: the
//! board lists a plan's chunks, the plan export their briefs. Any other error (no db,
//! a bad flag) passes through untouched — only the usage reject is answered.

/// The retype for `fael chunk list|ls …`, or `None` for any other argv or error.
pub(crate) fn retype(argv: &[String], e: &str) -> Option<String> {
    let (cmd, verb) = match argv {
        [c, v, ..] => (c.as_str(), v.as_str()),
        _ => return None,
    };
    if cmd != "chunk" || !matches!(verb, "list" | "ls") {
        return None;
    }
    if !e.contains("try 'fael chunk --help'") {
        return None;
    }
    Some(format!(
        "rejected: fael chunk {verb} — fael board --plan <plan> lists a plan's chunks · fael plan export <plan> their briefs"
    ))
}
