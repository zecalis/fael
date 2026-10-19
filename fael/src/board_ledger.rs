//! The ledger on the board (PLAN-fael-board l1, decision 01M4NCWEQ): a project's open issues,
//! decisions and notes, read-only, so the app shows them without calling `fael find` — a board
//! poll is not an agent pull, so nothing here records a find or marks a row seen (01M4MSMF).
//! fael shows facts only: `group` is `find --groups`' union-find over shared files, `signals`
//! holds `drifted` (doctor's rule: the row's files took 10+ commits since; one `git log`).
//! Which row is stale, and what to do with it, is the agent's call.
// ponytail: only the cheap signal rides every refresh. No `gone` (a git spawn per branch and
// file: ~0.5 s on this repo), no `landed` (doctor's [Shipped] needs `gh`, board stays offline),
// no `unused` (a parse of the machine's usage log) — `fael doctor --json` and `fael stats
// --rows` give them to the agent that reviews the ledger (l4).

use crate::{Repo, core};
use fael_core::plan::Fixer;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Default)]
pub(crate) struct Ledger {
    issues: Vec<Item>,
    decisions: Vec<Item>,
    notes: Vec<Item>,
}

#[derive(Serialize)]
struct Item {
    id: String,
    title: Option<String>,
    text: String,
    key: Option<String>,
    files: Vec<String>,
    at: String,
    by: String,
    to: Option<String>,
    from: Option<String>,
    revisit: Option<String>,
    /// the branch that claimed this issue (`fael claim`)
    held: Option<String>,
    /// issues only: the index of its shared-file group, groups largest first
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<usize>,
    /// issues only: the newest chunk linked to it by `chunk add --issue` (l2)
    #[serde(skip_serializing_if = "Option::is_none")]
    chunk: Option<Fixer>,
    signals: Vec<&'static str>,
}

/// Every open row of `r`'s log, by kind; issues in group order. `fixers` = issue id → its
/// newest linked chunk (`Store::fixers`), found through the issue's older versions too.
pub(crate) fn read(r: &Repo, fixers: &HashMap<String, Fixer>) -> Ledger {
    // under --watch, parsed once until the jsonl or the refs move (drift is a `git log`)
    let parsed = super::watch::ledger(&r.root, || {
        let log = crate::read(r);
        let drifted = crate::maintain::drift::ids(&log, &r.root);
        (log, drifted)
    });
    let (log, drifted) = &*parsed;
    let open = core::find(log, &core::Filter::default());
    // a re-filed issue (`--supersedes`, self-heal) has a new id: walk back to the one linked
    let pred: HashMap<&str, &str> = core::successors(log)
        .into_iter()
        .map(|(old, new)| (new, old))
        .collect();
    let fixer = |id: &str| {
        let mut id = id;
        // bounded: a reverted edge pair could loop
        for _ in 0..=pred.len() {
            if let Some(f) = fixers.get(id) {
                return Some(f.clone());
            }
            id = pred.get(id)?;
        }
        None
    };
    let item = |row: &core::Row, group: Option<usize>| Item {
        id: row.id.clone(),
        title: row.title.clone(),
        text: row.text.clone(),
        key: row.key.clone(),
        files: row.files.clone(),
        at: row.ts.clone(),
        by: row.by.clone(),
        to: row.to.clone(),
        from: row.from.clone(),
        revisit: row.revisit.clone(),
        held: row.held().map(String::from),
        chunk: group.and_then(|_| fixer(&row.id)),
        group,
        signals: drifted
            .contains(&row.id)
            .then_some("drifted")
            .into_iter()
            .collect(),
    };
    let of = |kind: &'static str| open.iter().filter(move |r| r.kind == kind).copied();
    let issues: Vec<&core::Row> = of("issue").collect();
    Ledger {
        issues: core::groups(&issues)
            .iter()
            .enumerate()
            .flat_map(|(g, rows)| rows.iter().map(move |r| (g, *r)))
            .map(|(g, r)| item(r, Some(g)))
            .collect(),
        decisions: of("decision").map(|r| item(r, None)).collect(),
        notes: of("note").map(|r| item(r, None)).collect(),
    }
}

impl Ledger {
    /// The chunks in `done` while an issue they fix is still open (needs you `issue-open`):
    /// fael never closes the issue — the agent or the owner does, once the fix holds.
    pub(crate) fn open_done(&self) -> Vec<String> {
        self.issues
            .iter()
            .filter_map(|i| i.chunk.as_ref())
            .filter(|c| c.state == "done")
            .map(|c| c.uid.clone())
            .collect()
    }
}
