//! Observation: Evidence and Candidate (chunk 1 of PLAN-fael-selfheal-verdict).
//!
//! Every open row becomes a Candidate carrying Evidence — the full observation
//! across every dimension Policy can read — before anything is decided. A
//! missing signal is a recorded fact (`Neither`, `No`), never an absence the
//! decider must interpret, so no if-chain can sneak back in through an
//! unasked question.

use crate::core;
use std::collections::BTreeSet;

/// Key relationship between the new row and a candidate — computed from the
/// key the caller sent only. A key fael guessed (auto-key, after heal) never
/// reaches here, so it can never close a row; chunk 2 holds this by type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum KeyRel {
    Same,
    Differ { old: String, new: String },
    OnlyNew { new: String },
    OnlyOld { old: String },
    Neither,
}

/// Two-answer question — writer, branch and kind each ask exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rel {
    Same,
    Other,
}

/// Text relationship — whitespace-collapsed equality, so a re-wrap does not
/// read as a new finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextRel {
    Same,
    Differ,
}

/// File overlap in raw counts — shared, of the new row, of the candidate.
/// Never a percentage: the decider reads the numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Overlap {
    pub shared: usize,
    pub of_new: usize,
    pub of_old: usize,
}

/// How the new row's text names a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameRel {
    /// An id after a "supersede*" word — the only naming that can close.
    AfterSupersede,
    /// An id mentioned in passing — observed, never acted on.
    Mentioned,
    No,
}

/// The complete observation of one open row against the row being added.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Evidence {
    pub key: KeyRel,
    pub writer: Rel,
    pub branch: Rel,
    pub kind: Rel,
    pub text: TextRel,
    pub files: Overlap,
    pub named: NameRel,
}

/// An open row plus what was observed about it.
pub(crate) struct Candidate<'a> {
    pub row: &'a core::Row,
    pub evidence: Evidence,
}

/// Variant names for the dry-run JSON — the shape of the evidence, never the
/// full structs (agents pay per byte on that path).
impl KeyRel {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            KeyRel::Same => "Same",
            KeyRel::Differ { .. } => "Differ",
            KeyRel::OnlyNew { .. } => "OnlyNew",
            KeyRel::OnlyOld { .. } => "OnlyOld",
            KeyRel::Neither => "Neither",
        }
    }
}

impl Rel {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Rel::Same => "Same",
            Rel::Other => "Other",
        }
    }
}

impl TextRel {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            TextRel::Same => "Same",
            TextRel::Differ => "Differ",
        }
    }
}

impl NameRel {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            NameRel::AfterSupersede => "AfterSupersede",
            NameRel::Mentioned => "Mentioned",
            NameRel::No => "No",
        }
    }
}

/// Observe every open row: one Candidate each, no decision yet.
pub(crate) fn observe<'a>(
    log: &'a core::Log,
    open: &[&'a core::Row],
    st: &core::Stamp,
    row: &core::Row,
) -> Vec<Candidate<'a>> {
    let named = text_targets(log, open, &row.text);
    let mentioned = mentions(log, open, &row.text);
    open.iter()
        .map(|&r| Candidate {
            row: r,
            evidence: Evidence {
                key: key_rel(row.key.as_deref(), r.key.as_deref()),
                writer: rel_eq(&r.by, &st.by),
                branch: rel_opt(r.branch(), st.branch.as_deref()),
                kind: rel_eq(&r.kind, &row.kind),
                text: text_rel(&r.text, &row.text),
                files: overlap(&r.files, &row.files),
                named: name_rel(&named, &mentioned, &r.id),
            },
        })
        .collect()
}

/// The (c) question for an `issue`: the same finding re-filed — same words
/// and a shared file. Two distinct issues on one topic share the key but not
/// their text, so neither may disappear.
pub(crate) fn same_finding(ev: &Evidence) -> bool {
    ev.text == TextRel::Same && ev.files.shared > 0
}

/// The one key open rows on these files already use — chunk 3e, and only when
/// exactly one key exists: zero candidates files the row keyless, several file
/// it keyless too (§6e), because a guessed key must never pick between topics
/// and never ask which. Deterministic: the keys are a sorted set. Runs after
/// `heal`, so this key never feeds (c).
pub(crate) fn auto_key(log: &core::Log, files: &[String]) -> Option<String> {
    let keys: BTreeSet<&str> = open_rows(log)
        .iter()
        .filter(|r| r.files.iter().any(|f| files.contains(f)))
        .filter_map(|r| r.key.as_deref())
        .collect();
    match keys.len() {
        1 => keys.into_iter().next().map(String::from),
        _ => None,
    }
}

/// Open rows: neither closed nor superseded, never a carrier (an event,
/// not a row to replace). Self-heal only ever touches these.
pub(crate) fn open_rows(log: &core::Log) -> Vec<&core::Row> {
    let hide: BTreeSet<&str> = core::closed(log)
        .union(&core::superseded(log))
        .copied()
        .collect();
    let open = |r: &&core::Row| !core::is_carrier_row(r) && !hide.contains(r.id.as_str());
    log.rows.iter().filter(open).collect()
}

fn rel_eq(a: &str, b: &str) -> Rel {
    if a == b { Rel::Same } else { Rel::Other }
}

fn rel_opt(a: Option<&str>, b: Option<&str>) -> Rel {
    if a == b { Rel::Same } else { Rel::Other }
}

fn key_rel(new_key: Option<&str>, old_key: Option<&str>) -> KeyRel {
    match (new_key, old_key) {
        (Some(n), Some(o)) if n == o => KeyRel::Same,
        (Some(n), Some(o)) => KeyRel::Differ {
            old: o.to_string(),
            new: n.to_string(),
        },
        (Some(n), None) => KeyRel::OnlyNew { new: n.to_string() },
        (None, Some(o)) => KeyRel::OnlyOld { old: o.to_string() },
        (None, None) => KeyRel::Neither,
    }
}

fn text_rel(a: &str, b: &str) -> TextRel {
    if norm_text(a) == norm_text(b) {
        TextRel::Same
    } else {
        TextRel::Differ
    }
}

fn overlap(old: &[String], new: &[String]) -> Overlap {
    Overlap {
        shared: old.iter().filter(|f| new.contains(f)).count(),
        of_new: new.len(),
        of_old: old.len(),
    }
}

fn name_rel(named: &[&core::Row], mentioned: &BTreeSet<&str>, id: &str) -> NameRel {
    if named.iter().any(|r| r.id == id) {
        NameRel::AfterSupersede
    } else if mentioned.contains(id) {
        NameRel::Mentioned
    } else {
        NameRel::No
    }
}

/// Whitespace-collapsed text, so a re-wrap does not read as a new finding.
fn norm_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Open rows the text names after a "supersede*" word. The word still leads
/// the id in the broken-flag rescue — the caller asked to supersede something,
/// but a text that only mentions a row in passing ("see 01A… for context")
/// must not close it (§6d). The word arms only the ids right after it ("Supersedes
/// A and B"); any other word disarms, so an id cited later is a mention. Ids
/// of rows already closed or superseded are not targets: naming them changes nothing.
fn text_targets<'a>(log: &'a core::Log, open: &[&'a core::Row], text: &str) -> Vec<&'a core::Row> {
    let mut out: Vec<&core::Row> = vec![];
    let mut armed = false;
    for tok in text.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric());
        if t.is_empty() {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("supersede") || !armed || lower == "and" || lower == "or" {
            armed |= lower.starts_with("supersede");
            continue;
        }
        // resolution is exact-id or unique prefix, same as --supersedes, so a
        // word that merely looks like an id resolves to nothing and disarms
        let Ok(r) = core::resolve_row(log, t) else {
            armed = false;
            continue;
        };
        if open.iter().any(|o| o.id == r.id) && !out.iter().any(|o| o.id == r.id) {
            out.push(r);
        }
    }
    out
}

/// Open rows the text resolves to without a leading "supersede*" word —
/// observed as Mentioned, never acted on. Same resolution as --supersedes,
/// so only real ids count, never words that merely look like one.
fn mentions<'a>(log: &'a core::Log, open: &[&'a core::Row], text: &str) -> BTreeSet<&'a str> {
    let mut out = BTreeSet::new();
    for tok in text.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric());
        if t.is_empty() {
            continue;
        }
        let Ok(r) = core::resolve_row(log, t) else {
            continue;
        };
        if open.iter().any(|o| o.id == r.id) {
            out.insert(r.id.as_str());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        id: &str,
        by: &str,
        kind: &str,
        text: &str,
        files: &[&str],
        key: Option<&str>,
    ) -> core::Row {
        let mut r = core::Row::new(
            by,
            kind,
            text,
            files.iter().map(|s| s.to_string()).collect(),
        );
        r.id = id.to_string();
        r.key = key.map(String::from);
        r
    }

    fn stamp() -> core::Stamp {
        core::Stamp {
            by: "me".to_string(),
            branch: Some("main".to_string()),
            sha: None,
        }
    }

    #[test]
    fn key_rel_covers_all_five_cases() {
        assert_eq!(key_rel(Some("a"), Some("a")), KeyRel::Same);
        assert_eq!(
            key_rel(Some("n"), Some("o")),
            KeyRel::Differ {
                old: "o".into(),
                new: "n".into()
            }
        );
        assert_eq!(
            key_rel(Some("n"), None),
            KeyRel::OnlyNew { new: "n".into() }
        );
        assert_eq!(
            key_rel(None, Some("o")),
            KeyRel::OnlyOld { old: "o".into() }
        );
        assert_eq!(key_rel(None, None), KeyRel::Neither);
    }

    #[test]
    fn overlap_keeps_raw_counts() {
        let o = overlap(
            &["a".to_string(), "b".to_string()],
            &["b".to_string(), "c".to_string()],
        );
        assert_eq!(
            o,
            Overlap {
                shared: 1,
                of_new: 2,
                of_old: 2
            }
        );
    }

    #[test]
    fn observe_records_every_dimension() {
        let a = row(
            "01AAAAAAAAAAAAAAAAAAAAAAAAAA",
            "me",
            "note",
            "first pass",
            &["src/a.rs"],
            Some("k:1"),
        );
        let b = row(
            "01BBBBBBBBBBBBBBBBBBBBBBBBBB",
            "other",
            "decision",
            "other words here",
            &["src/b.rs"],
            None,
        );
        let log = core::Log {
            rows: vec![a, b],
            ..Default::default()
        };
        let st = stamp();
        let new = core::Row::new(
            "me",
            "note",
            "see 01BBBBBBBBBBBBBBBBBBBBBBBBBB; second. Supersedes 01AAAAAAAAAAAAAAAAAAAAAAAAAA",
            vec!["src/a.rs".to_string(), "src/c.rs".to_string()],
        );
        let open = open_rows(&log);
        let cands = observe(&log, &open, &st, &new);
        assert_eq!(cands.len(), 2);
        let ea = &cands[0].evidence;
        assert_eq!(ea.named, NameRel::AfterSupersede);
        assert_eq!(ea.writer, Rel::Same);
        assert_eq!(ea.branch, Rel::Other);
        assert_eq!(ea.kind, Rel::Same);
        assert_eq!(ea.key, KeyRel::OnlyOld { old: "k:1".into() });
        assert_eq!(ea.text, TextRel::Differ);
        assert_eq!(ea.files.shared, 1);
        assert_eq!(ea.files.of_new, 2);
        assert_eq!(ea.files.of_old, 1);
        let eb = &cands[1].evidence;
        assert_eq!(eb.named, NameRel::Mentioned);
        assert_eq!(eb.writer, Rel::Other);
        assert_eq!(eb.kind, Rel::Other);
        assert_eq!(eb.key, KeyRel::Neither);
        assert_eq!(eb.text, TextRel::Differ);
        assert_eq!(eb.files.shared, 0);
    }
}
