//! `bump_row` — change routing/urgency/revisit on an open row as a new
//! version (MVCC-style). Split out of `append.rs` at the 400-line ratchet;
//! the actual write stays in `append::add_row`.

use super::Log;
use super::append::add_row;
use crate::{
    Config, Row, Stamp, Urgent, UrgentChange, closed, resolve, resolve_urgent, superseded,
};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// What `bump_row` changes — bundled so the arg count stays under the lint
/// (the binary's `AddOpts` does the same for `add_row`). `to`/`revisit`:
/// `None` keeps the old value, `Some("")` clears it, anything else sets it.
/// `held` (`fael claim`): `Some(branch)` sets it, `None` keeps the old one.
/// `fh`: `Some(map)` restamps the file hashes from disk (`fael bump`);
/// `None` leaves the new row without any (`fael claim` passes the old map
/// itself, so a claim never looks like a check).
pub struct BumpOpts {
    pub to: Option<String>,
    pub urgent: UrgentChange,
    pub revisit: Option<String>,
    pub held: Option<String>,
    pub fh: Option<Map<String, Value>>,
}

/// The same kind, title, text, files and key, new `to`/`urgent`/`revisit`,
/// superseding the old row — the one add path every adapter (CLI, MCP, a
/// server) goes through, so the old version hides through `superseded()` with
/// no new visibility rule. Text and files never change through bump — file a
/// new row for new content.
pub fn bump_row(
    fael: &Path,
    journal: Option<&Path>,
    log: &Log,
    cfg: &Config,
    stamp: &Stamp,
    id: &str,
    opts: BumpOpts,
) -> Result<(Row, PathBuf, Vec<String>), String> {
    let old = resolve(log, id)?.clone();
    if closed(log).contains(old.id.as_str()) {
        return Err(format!(
            "rejected: {} is already closed — bump an open row",
            old.id
        ));
    }
    if superseded(log).contains(old.id.as_str()) {
        return Err(format!(
            "rejected: {} is already superseded — bump the newer version",
            old.id
        ));
    }
    let to = match opts.to {
        None => old.to.clone(),
        Some(t) => {
            let t = t.trim().to_lowercase();
            (!t.is_empty()).then_some(t)
        }
    };
    let urgent = match opts.urgent {
        UrgentChange::Keep => old.urgent,
        UrgentChange::End => resolve_urgent(log, &Urgent::End)?,
        UrgentChange::Before(t) => resolve_urgent(log, &Urgent::Before(t))?,
        UrgentChange::Remove => None,
    };
    let revisit = match opts.revisit {
        None => old.revisit.clone(),
        Some(v) => {
            let v = v.trim().to_string();
            (!v.is_empty()).then_some(v)
        }
    };
    let mut row = Row::new(&stamp.by, &old.kind, &old.text, old.files.clone());
    row.key = old.key.clone();
    row.title = old.title.clone();
    row.to = to;
    row.urgent = urgent;
    row.revisit = revisit;
    if let Some(h) = opts.held.as_deref().or(old.held()) {
        row.extra.insert("held".into(), h.into());
    }
    if let Some(fh) = opts.fh
        && !fh.is_empty()
    {
        row.extra.insert("fh".into(), Value::Object(fh));
    }
    add_row(fael, journal, log, cfg, stamp, row, Some(&old.id))
}
