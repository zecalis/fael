//! Joint start (plan overlap-broker o2, issue 01M5PB7Z): `chunk start <uid> --with <partner>
//! --as pair|lane|follow` turns a clash into a handshake recorded on the run row and the start
//! event — crew.db only, no daemon. Judges nothing: the caller names the mode; `--force` stays.

use super::chunk::Row;
use super::holder::{clash_report, holder_detail};
use super::start::Start;
use super::store::err;
use fael_core::OverlapClass;
use rusqlite::{OptionalExtension, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    /// one R, the partner's worktree and branch
    Pair,
    /// own worktree; a dir-level overlap is no refusal, the owner merges in any order
    Lane,
    /// read-only: no clash against the partner, its handoff reaches the brief by file
    Follow,
}

impl Joint {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "pair" => Ok(Self::Pair),
            "lane" => Ok(Self::Lane),
            "follow" => Ok(Self::Follow),
            _ => Err(format!("rejected: --as {s} is not pair, lane or follow")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pair => "pair",
            Self::Lane => "lane",
            Self::Follow => "follow",
        }
    }
}

/// The partner's live run: what a pair start joins.
pub(super) struct Partner {
    pub uid: String,
    pub mode: Joint,
    pub run: String,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
}

impl Partner {
    /// the event / brief line that records the handshake
    pub fn text(&self) -> String {
        format!(
            "joint {} with {} (run {})",
            self.mode.as_str(),
            self.uid,
            self.run
        )
    }
}

/// `--with`/`--as` → the partner's live run. `None` = a solo start.
fn partner(
    tx: &Transaction,
    first: &Row,
    ms: &[String],
    o: &Start,
) -> Result<Option<Partner>, String> {
    let (with, mode) = match (&o.with, o.joint) {
        (None, None) => return Ok(None),
        (Some(w), Some(m)) => (w, m),
        _ => return Err("rejected: --with and --as go together".into()),
    };
    let p = super::lookup::lookup(tx, with)?.ok_or_else(|| super::lookup::no_chunk(with))?;
    let uid = p.1.clone();
    if p.0 == first.id || ms.contains(&uid) {
        return Err(format!(
            "rejected: --with {with} is this chunk (or its pair) — name another"
        ));
    }
    let Some((run, _)) = holder_detail(tx, p.0)? else {
        return Err(format!(
            "rejected: --with {with} has no live run — start {with} first, or start this one alone"
        ));
    };
    let (worktree, branch, base) = tx
        .query_row(
            "SELECT worktree, branch, base FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
            [p.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(err)?
        .unwrap_or_default();
    Ok(Some(Partner {
        uid,
        mode,
        run,
        worktree,
        branch,
        base,
    }))
}

/// The start's clash gate. A clash with the partner is no refusal for `follow`, `pair`, or a
/// `lane` over a dir (prefix) overlap; a lane over the same file still refuses — that is a
/// pair. Any other clash (or none named) refuses with the ready joint command, `--force` last.
pub(super) fn gate(
    tx: &Transaction,
    first: &Row,
    ms: &[String],
    o: &Start,
) -> Result<Option<Partner>, String> {
    let p = partner(tx, first, ms, o)?;
    let report = clash_report(tx, ms, &o.shared, &o.stale)?;
    let exact_lane = |u: &str, c: OverlapClass| {
        p.as_ref()
            .is_some_and(|p| p.uid == u && p.mode == Joint::Lane && c == OverlapClass::Exact)
    };
    let kept: Vec<_> = report
        .iter()
        .filter(|(u, c, _)| {
            !p.as_ref()
                .is_some_and(|p| &p.uid == u && !exact_lane(u, *c))
        })
        .collect();
    if kept.is_empty() || o.force {
        return Ok(p);
    }
    let uid = &first.uid;
    let (hit, class, _) = kept[0];
    let how = if kept.iter().any(|(_, c, _)| *c == OverlapClass::Exact) || exact_lane(hit, *class) {
        "pair"
    } else {
        "lane"
    };
    let lines: Vec<_> = kept.iter().map(|k| k.2.as_str()).collect();
    Err(format!(
        "rejected: chunk {uid} shares files with a running chunk — {}; try `fael chunk start {uid} --with {hit} --as {how}` to start beside it, or `fael chunk start {uid} --force` to start it anyway",
        lines.join("; ")
    ))
}

#[cfg(test)]
#[path = "joint_tests.rs"]
mod tests;
