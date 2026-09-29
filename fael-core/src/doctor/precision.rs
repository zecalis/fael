//! Per-rule self-heal precision from restore labels
//! (PLAN-fael-selfheal-restore chunk 3).
//!
//! A `restores` row is a human label: the edge it reverts hid a row that
//! should live, so the act was wrong — until an explicit re-supersede of the
//! restored target after the restore (`caller:flag` / `explicit:text`)
//! overturns it (the human re-confirmed the hide, so the act was right).
//! Anything the auto-healer files alone (`identity:key`,
//! `heuristic:files`) is a prediction, never a label: a re-add the healer
//! handles neither labels an edge nor overturns one. Edges whose superseder
//! carries no known `decision_source` (pre-verdict rows read as `unknown`)
//! are skipped entirely — `doctor` never counts them into precision.

use super::{Kind, Problem};
use crate::{Log, abbrev};
use std::collections::HashMap;

/// The rule a `decision_source` belongs to — `None` for rows the verdict
/// plan predates (absent reads as `unknown`, never backfilled, never
/// counted). `:cross-key` is exposure, not a rule, so it strips off.
fn base(source: Option<&str>) -> Option<&str> {
    let s = source?;
    Some(s.strip_suffix(":cross-key").unwrap_or(s))
}

/// Explicit human signals — labels, never predictions. Only these overturn a
/// restore label; an auto-healer re-supersede is one more prediction.
fn is_explicit(source: Option<&str>) -> bool {
    matches!(base(source), Some("caller:flag" | "explicit:text"))
}

/// Edge (superseder id) → latest restore row id. Ids are ULIDs, so string
/// order is filing order — "after the restore" is one comparison.
fn labels(log: &Log) -> HashMap<&str, &str> {
    let mut out: HashMap<&str, &str> = HashMap::new();
    for r in &log.rows {
        if let Some(e) = r.restores.as_deref() {
            out.entry(e)
                .and_modify(|id| *id = (*id).max(r.id.as_str()))
                .or_insert(r.id.as_str());
        }
    }
    out
}

/// The restored target was explicitly hidden again after the restore — the
/// human re-confirmed the hide, so the original act counts as right.
fn overturned(log: &Log, edge: &str, target: &str, after: &str) -> bool {
    log.rows.iter().any(|x| {
        x.id.as_str() != edge
            && x.supersedes.as_deref() == Some(target)
            && is_explicit(x.decision_source.as_deref())
            && x.id.as_str() > after
    })
}

/// Per-rule precision over labeled edges only — `None` when no label lands
/// on a known-source edge (no labels, re-adds alone, or pre-verdict edges
/// only). Unlabeled edges are reported as not counted, never as correct.
pub fn precision(log: &Log) -> Option<Problem> {
    let lab = labels(log);
    if lab.is_empty() {
        return None;
    }
    // rule → (right, wrong); full edge ids per rule for the examples
    let mut per: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut egs: HashMap<&str, Vec<(String, bool)>> = HashMap::new();
    let (mut unlabeled, mut unknown) = (0usize, 0usize);
    for r in &log.rows {
        let Some(target) = r.supersedes.as_deref() else {
            continue;
        };
        let Some(rule) = base(r.decision_source.as_deref()) else {
            unknown += lab.contains_key(r.id.as_str()) as usize;
            continue;
        };
        let Some(after) = lab.get(r.id.as_str()) else {
            unlabeled += 1;
            continue;
        };
        let right = overturned(log, &r.id, target, after);
        let e = per.entry(rule).or_insert((0, 0));
        if right {
            e.0 += 1;
        } else {
            e.1 += 1;
        }
        egs.entry(rule).or_default().push((r.id.clone(), right));
    }
    if per.is_empty() {
        return None;
    }
    let w = abbrev(log);
    let mut rules: Vec<&&str> = per.keys().collect();
    rules.sort_unstable();
    let mut parts = vec![];
    let mut ids: Vec<String> = vec![];
    for rule in rules {
        let (right, wrong) = per[rule];
        let mut eg = egs[rule].clone();
        eg.sort();
        let shown: Vec<String> = eg
            .iter()
            .take(5)
            .map(|(id, ok)| {
                format!(
                    "{} {}",
                    w.short(id),
                    if *ok { "re-confirmed" } else { "still open" }
                )
            })
            .collect();
        ids.extend(eg.iter().map(|(id, _)| id.clone()));
        parts.push(format!(
            "{rule} {right}/{} correct ({})",
            right + wrong,
            shown.join("; ")
        ));
    }
    let mut tail = format!("{unlabeled} unlabeled edge(s) not counted");
    if unknown > 0 {
        tail.push_str(&format!(", {unknown} pre-verdict edge(s) skipped"));
    }
    ids.sort();
    Some(
        Problem::info(
            Kind::Precision,
            format!(
                "self-heal precision per rule (restore labels only — re-adds never label): {}; {tail}",
                parts.join("; ")
            ),
        )
        .with_ids(ids),
    )
}
