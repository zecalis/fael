//! `fael add <kind> --supersedes <id> --replace "<old>" --with "<new>"` —
//! re-file a row with one passage changed, instead of retyping the whole body.
//! The new row takes the old one's text, files, key and title; `--files`,
//! `--key` and `--title` still override. Never a guess: `<old>` must occur in
//! the body exactly once, else the add is rejected.

use crate::{Args, Repo, core};

/// What the new row inherits from the one it replaces.
pub(crate) struct Base {
    pub text: String,
    pub files: Vec<String>,
    pub key: Option<String>,
    pub title: Option<String>,
}

pub(crate) fn base(r: &Repo, a: &Args, kind: &str) -> Result<Base, String> {
    let id = a.one("supersedes").ok_or(
        "rejected: --replace edits the body of the row --supersedes names — add --supersedes <id>",
    )?;
    let old = a.one("replace").unwrap_or_default();
    let new = a
        .one("with")
        .ok_or("rejected: --replace needs --with \"<new text>\"")?;
    if old.is_empty() {
        return Err("rejected: --replace needs the text to replace, not an empty string".into());
    }
    let log = crate::read(r);
    let row = core::resolve_row(&log, &id)?;
    if row.kind != kind {
        return Err(format!(
            "rejected: {id} is a {} — add {} with --replace, or retype the body to change its kind",
            row.kind, row.kind
        ));
    }
    match row.text.matches(old.as_str()).count() {
        1 => Ok(Base {
            text: row.text.replacen(&old, &new, 1),
            files: row.files.clone(),
            key: row.key.clone(),
            title: row.title.clone(),
        }),
        0 => Err(format!(
            "rejected: {old:?} is not in the body of {id} — copy it exactly from fael find {id}"
        )),
        n => Err(format!(
            "rejected: {old:?} occurs {n} times in the body of {id} — lengthen it until it matches once"
        )),
    }
}
