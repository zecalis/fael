//! `fael restore`: revert a supersede edge with an event row (`restores`), never
//! by rewriting. Split out of `append` (file-size ratchet) — the edge math
//! stays in `select::{reverted, superseded}`, this module only resolves which
//! edge and appends the row.

use super::{Log, write_both};
use crate::{Config, Row, Stamp, abbrev, append, closed, resolve_row, reverted, validate_restore};
use std::path::{Path, PathBuf};

/// What `restore_row` did. `row`/`path` are `Some` only when an edge was
/// actually reverted — an idempotent repeat writes nothing. `message` is the
/// one line the CLI prints either way; `warns` are non-fatal write warnings.
pub struct Restored {
    pub row: Option<Row>,
    pub path: Option<PathBuf>,
    pub message: String,
    pub warns: Vec<String>,
}

/// Revert one supersede edge: `target` (`fael restore <B>`) picks the edge by
/// its target, or names the superseder directly (`fael restore <A>` reverts
/// A's edge). Several still-active edges into one target reject and name the
/// superseder ids to pass positionally. An
/// already-open row, or an already-reverted edge, is info — exit 0, nothing
/// written — never an error.
pub fn restore_row(
    fael: &Path,
    journal: Option<&Path>,
    log: &Log,
    cfg: &Config,
    stamp: &Stamp,
    target: Option<&str>,
    edge: Option<&str>,
) -> Result<Restored, String> {
    let short = abbrev(log);
    let s = |id: &str| short.short(id).to_string();
    match pick_edge(log, target, edge)? {
        Pick::Open(b) => Ok(Restored {
            row: None,
            path: None,
            message: format!("{} is already open — nothing to restore", s(&b)),
            warns: vec![],
        }),
        Pick::Revert(e, b) if reverted(log).contains(e.as_str()) => Ok(Restored {
            row: None,
            path: None,
            message: format!(
                "{} is already restored (edge {} already reverted)",
                s(&b),
                s(&e)
            ),
            warns: vec![],
        }),
        Pick::Revert(e, b) => {
            let mut row = Row::restored(&stamp.by, &e, &b);
            stamp.apply(&mut row);
            let msg = if closed(log).contains(b.as_str()) {
                format!(
                    "{} restored — supersede by {} reverted (still closed)",
                    s(&b),
                    s(&e)
                )
            } else {
                format!("{} restored — supersede by {} reverted", s(&b), s(&e))
            };
            let path = write_both(fael, journal, |d| {
                validate_restore(&row, cfg)?;
                append(d, &row, false)
            })?;
            Ok(Restored {
                row: Some(row),
                path: Some(path),
                message: msg,
                warns: vec![],
            })
        }
    }
}

/// Either the row is already open, or the `(edge, target)` pair to revert —
/// both as full ids.
enum Pick {
    Open(String),
    Revert(String, String),
}

fn pick_edge(log: &Log, target: Option<&str>, edge: Option<&str>) -> Result<Pick, String> {
    match (target, edge) {
        (None, None) => Err("rejected: restore needs a row — `fael restore <id>`".into()),
        (Some(t), None) => {
            let id = resolve_row(log, t)?.id.clone();
            match active_edges_into(log, &id).as_slice() {
                // hidden behind one edge: revert it; behind several: name one
                // superseder positionally — `fael restore <edge-id>`
                [_] => edge_into(log, t),
                [_, _, ..] => {
                    let es = active_edges_into(log, &id);
                    Err(format!(
                        "{} edges still supersede {id} ({}) — revert one with `fael restore <edge-id>`",
                        es.len(),
                        es.join(", ")
                    ))
                }
                // not hidden: a superseder id names its own edge directly
                [] => match edge_from(log, t) {
                    Ok((a, b)) => Ok(Pick::Revert(a, b)),
                    Err(_) => Ok(Pick::Open(id)),
                },
            }
        }
        (None, Some(e)) => edge_from(log, e).map(|(a, b)| Pick::Revert(a, b)),
        (Some(t), Some(e)) => {
            let (a, b) = edge_from(log, e)?;
            let want = resolve_row(log, t)?.id.clone();
            if b != want {
                return Err(format!(
                    "rejected: {e} reverts {b}, not {want} — pick one id"
                ));
            }
            Ok(Pick::Revert(a, b))
        }
    }
}

/// The still-active edges into `target`, oldest first.
fn active_edges_into(log: &Log, target: &str) -> Vec<String> {
    let rev = reverted(log);
    let mut active: Vec<String> = log
        .rows
        .iter()
        .filter(|r| r.supersedes.as_deref() == Some(target) && !rev.contains(r.id.as_str()))
        .map(|r| r.id.clone())
        .collect();
    active.sort_unstable();
    active
}

/// The still-active edge into `target` — or `Open` when none is left.
fn edge_into(log: &Log, target: &str) -> Result<Pick, String> {
    let b = resolve_row(log, target)?.id.clone();
    match active_edges_into(log, &b).as_slice() {
        [] => Ok(Pick::Open(b)),
        [e] => Ok(Pick::Revert(e.clone(), b)),
        es => Err(format!(
            "{} edges still supersede {b} ({}) — revert one with `fael restore <edge-id>`",
            es.len(),
            es.join(", ")
        )),
    }
}

/// The edge a superseder id names — the superseder plus its target. A row
/// that supersedes nothing names no edge.
fn edge_from(log: &Log, edge: &str) -> Result<(String, String), String> {
    let a = resolve_row(log, edge)?;
    match a.supersedes.as_deref() {
        Some(b) => Ok((a.id.clone(), b.to_string())),
        None => Err(format!(
            "rejected: {} supersedes nothing — no edge to revert",
            a.id
        )),
    }
}
