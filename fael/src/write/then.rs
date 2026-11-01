//! `add … --then "<text>"` — the part still to do, filed with the row just
//! written: an issue that `follows` it, same files and key. A measured result
//! (a note) and its leftover work are then one call, and the issue holds only
//! the leftover — no result text to trip the closed-earlier recall.

use super::{AddOpts, add_row};
use crate::core;
use std::path::PathBuf;

/// The child issue of `parent`. A reject here leaves `parent` written, so the
/// error names it and the call that files the child alone.
pub(crate) fn child(
    r: &crate::Repo,
    parent: &core::Row,
    text: &str,
    force: bool,
) -> Result<(core::Row, PathBuf, Vec<String>), String> {
    let opts = AddOpts {
        key: parent.key.clone(),
        to: None,
        from: None,
        title: None,
        revisit: None,
        urgent: core::Urgent::Unset,
        supersedes: None,
        follows: Some(parent.id.clone()),
        force,
        by: None,
    };
    add_row(r, "issue", text, &parent.files, opts).map_err(|e| {
        format!(
            "{e}\n{} is written; file the rest alone: fael add issue \"<text>\" --follows {}",
            parent.id, parent.id
        )
    })
}
