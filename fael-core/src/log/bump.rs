//! `bump_row` — change routing/urgency/revisit on an open row under its own
//! id: one bump event appended, folded onto the row by `fold_bumps`. Split
//! out of `append.rs` at the 400-line ratchet.

use super::{Log, write_both};
use crate::{
    Config, Row, Stamp, Urgent, UrgentChange, append, closed, resolve_row, resolve_urgent,
    superseded, validate_bump,
};
use serde_json::{Map, Value};
use std::collections::HashMap;
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

/// The same row under the same id, new `to`/`urgent`/`revisit`: one bump
/// event appended (a carrier, `Row::bumped`) that `fold_bumps` lays onto the
/// row in memory — the one write path every adapter (CLI, MCP, a server)
/// goes through. The event is a snapshot of the moved fields, so the newest
/// alone decides. Text and files never change through bump — file a new row
/// for new content. Returns the row as folded.
pub fn bump_row(
    fael: &Path,
    journal: Option<&Path>,
    log: &Log,
    cfg: &Config,
    stamp: &Stamp,
    id: &str,
    opts: BumpOpts,
) -> Result<(Row, PathBuf, Vec<String>), String> {
    let old = resolve_row(log, id)?.clone();
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
    let mut ev = Row::bumped(&stamp.by, &old.id);
    // the fold orders events by id: a bump in the same ms as the row's last
    // one would sort by its random half, so step past it. ponytail: two
    // writers in one ms still order arbitrarily — deterministically, so
    // clones still converge
    let last = log
        .rows
        .iter()
        .filter(|r| r.bumps.as_deref() == Some(&old.id));
    if let Some(last) = last.map(|r| r.id.as_str()).max()
        && ev.id.as_str() <= last
    {
        ev.id = crate::ulid_at(crate::ulid_ms(last).unwrap_or(0) + 1);
    }
    ev.to = match opts.to {
        None => old.to.clone(),
        Some(t) => {
            let t = t.trim().to_lowercase();
            (!t.is_empty()).then_some(t)
        }
    };
    ev.urgent = match opts.urgent {
        UrgentChange::Keep => old.urgent,
        UrgentChange::End => resolve_urgent(log, &Urgent::End)?,
        UrgentChange::Before(t) => resolve_urgent(log, &Urgent::Before(t))?,
        UrgentChange::Remove => None,
    };
    ev.revisit = match opts.revisit {
        None => old.revisit.clone(),
        Some(v) => {
            let v = v.trim().to_string();
            (!v.is_empty()).then_some(v)
        }
    };
    // the queue holds issues — the check `validate` runs on an add row
    if old.kind != "issue" && ev.urgent.is_some() {
        return Err(
            "rejected: urgent is for issues — file it as kind issue or drop --urgent".into(),
        );
    }
    if let Some(h) = opts.held.as_deref().or(old.held()) {
        ev.extra.insert("held".into(), h.into());
    }
    if let Some(fh) = opts.fh
        && !fh.is_empty()
    {
        ev.extra.insert("fh".into(), Value::Object(fh));
    }
    stamp.apply(&mut ev);
    let (path, warns) = write_both(fael, journal, cfg, |d| {
        validate_bump(&ev, cfg)?;
        append(d, &ev, false)
    })?;
    let mut row = old;
    apply(&mut row, &ev);
    Ok((row, path, warns))
}

/// Lay every bump event onto the row it names, oldest first, so the newest
/// wins — the view every query reads. Raw readers (sync, compact) skip this:
/// they carry the events as rows and must never ship a folded row.
pub fn fold_bumps(mut log: Log) -> Log {
    let mut evs: Vec<Row> = log
        .rows
        .iter()
        .filter(|r| r.bumps.is_some())
        .cloned()
        .collect();
    if evs.is_empty() {
        return log;
    }
    evs.sort_by(|a, b| a.id.cmp(&b.id));
    let at: HashMap<String, usize> = log
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| !crate::is_carrier_row(r))
        .map(|(i, r)| (r.id.clone(), i))
        .collect();
    for ev in &evs {
        if let Some(&i) = ev.bumps.as_deref().and_then(|t| at.get(t)) {
            apply(&mut log.rows[i], ev);
        }
    }
    log
}

/// The moved fields from `ev` onto `row`: the snapshot ones (`to`, `urgent`,
/// `revisit`, `held`, `fh` — absent clears) and the write stamp (`ts`, plus
/// `sha`/`branch` when the bump had them), so a bump still reads as the
/// check it is to drift and the fh verdict. `id`, `by` and `session` stay
/// the row's own: who wrote it does not change.
fn apply(row: &mut Row, ev: &Row) {
    row.to = ev.to.clone();
    row.urgent = ev.urgent;
    row.revisit = ev.revisit.clone();
    row.ts = ev.ts.clone();
    for k in ["held", "fh"] {
        match ev.extra.get(k) {
            Some(v) => row.extra.insert(k.into(), v.clone()),
            None => row.extra.remove(k),
        };
    }
    for k in ["sha", "branch"] {
        if let Some(v) = ev.extra.get(k) {
            row.extra.insert(k.into(), v.clone());
        }
    }
}
