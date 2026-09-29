//! Renderer: Verdict → Heal (chunk 1 of PLAN-fael-selfheal-verdict).
//!
//! Every string below is byte-identical to what the if-chain printed —
//! chunk 1 separated the words from the decision without changing one byte.
//! Chunk 3 wraps cross-key acts in `CrossKey`: the inner lines print as one
//! `warning:` line, as info, or not at all, per `[selfheal] cross_key` — the
//! "also kept" trailer always stays info, so one add counts at most one ask.

use super::decide::Verdict;
use crate::core;

/// What self-heal decided: the supersedes value core should resolve (a
/// caller's flag only when it resolves to nothing — (d) then replaces it),
/// the provenance `write` stamps as `decision_source`, plus info lines.
pub(crate) struct Heal {
    pub supersedes: Option<String>,
    pub source: Option<String>,
    pub notes: Vec<String>,
}

pub(crate) fn render(
    v: &Verdict,
    row: &core::Row,
    st: &core::Stamp,
    w: &core::Abbrev,
    flag: Option<&str>,
    cross: core::CrossKey,
) -> Heal {
    let short = |id: &str| w.short(id).to_string();
    let k = row.key.as_deref().unwrap_or_default();
    match v {
        Verdict::FlagPassthrough | Verdict::FlagUnresolved => Heal {
            supersedes: flag.map(String::from),
            source: None,
            notes: vec![],
        },
        Verdict::FlagRescued { target } => Heal {
            supersedes: Some(target.clone()),
            source: None,
            notes: vec![format!(
                "--supersedes {:?} matched nothing; used {} from the text",
                flag.unwrap_or_default(),
                short(target)
            )],
        },
        Verdict::TextAct { target, also } => {
            let mut notes = vec![format!("superseded {} (id in the text)", short(target))];
            also_line(also, w, &mut notes);
            Heal {
                supersedes: Some(target.clone()),
                source: None,
                notes,
            }
        }
        Verdict::TextHold { targets } => Heal {
            supersedes: None,
            source: None,
            notes: vec![format!(
                "open rows {} are named in the text — kept all; pass --supersedes <id> to replace one",
                id_list(targets, w)
            )],
        },
        Verdict::KeyAct { target, also } => {
            let mut notes = vec![format!(
                "superseded {} (open {}, same key {k})",
                short(target),
                row.kind
            )];
            also_line(also, w, &mut notes);
            Heal {
                supersedes: Some(target.clone()),
                source: None,
                notes,
            }
        }
        Verdict::KeyIssueKept { targets } => Heal {
            supersedes: None,
            source: None,
            notes: vec![format!(
                "open issue {} uses key {k} — kept open (a different finding)",
                id_list(targets, w)
            )],
        },
        Verdict::KeyOtherWriter { target } => Heal {
            supersedes: None,
            source: None,
            notes: vec![format!(
                "open {} {} uses key {k} (another writer) — kept open",
                row.kind,
                short(target)
            )],
        },
        Verdict::KeyMany { targets } => Heal {
            supersedes: None,
            source: None,
            notes: vec![format!(
                "open rows {} already use key {k} — kept all; pass --supersedes <id> to replace one",
                id_list(targets, w)
            )],
        },
        Verdict::FilesAct { target } => Heal {
            supersedes: Some(target.clone()),
            source: None,
            notes: vec![match &st.branch {
                Some(b) => format!(
                    "superseded {} (open note, same branch {b}, same files)",
                    short(target)
                ),
                None => format!("superseded {} (open note, same files)", short(target)),
            }],
        },
        Verdict::FilesMany { targets } => Heal {
            supersedes: None,
            source: None,
            notes: vec![format!(
                "open notes {} overlap these files — kept all; pass --supersedes <id> to replace one",
                id_list(targets, w)
            )],
        },
        Verdict::CrossKey { inner, old, new } => {
            expose(render(inner, row, st, w, flag, cross), old, new, cross)
        }
        Verdict::Noop => Heal {
            supersedes: None,
            source: None,
            notes: vec![],
        },
    }
}

/// Cross-key exposure (chunk 3): the inner act stands under every mode —
/// Warn prefixes its first line as the one `warning:` of this add (the
/// also-kept trailer stays info, so one add counts at most one ask), Info
/// keeps the lines, Off clears them.
fn expose(mut h: Heal, old: &str, new: &str, cross: core::CrossKey) -> Heal {
    match cross {
        core::CrossKey::Warn => {
            if let Some(first) = h.notes.first_mut() {
                *first = format!("warning: {first} — key {old} → {new}");
            }
        }
        core::CrossKey::Info => {}
        core::CrossKey::Off => h.notes.clear(),
    }
    h
}

/// The "also overlaps" trailer after an act on a note — absent when no other
/// note overlaps, so single-match acts print exactly one line.
fn also_line(also: &[String], w: &core::Abbrev, out: &mut Vec<String>) {
    if !also.is_empty() {
        out.push(format!(
            "note {} also overlaps these files — kept open",
            id_list(also, w)
        ));
    }
}

/// Up to 5 ids shortened by `w` (`core::abbrev`), then `(+N more)`. Never a
/// fixed `[..8]`: a ULID's leading characters are its millisecond timestamp, so
/// rows written in the same second share them and `--supersedes <prefix>`
/// would be ambiguous — `Abbrev::short` lengthens a prefix until it is unique.
/// Sorted before the cut, so *which* five print never depends on log order.
fn id_list(ids: &[String], w: &core::Abbrev) -> String {
    let mut s: Vec<&str> = ids.iter().map(|id| w.short(id)).collect();
    s.sort_unstable();
    let n = ids.len();
    s.truncate(5);
    let mut out = s.join(", ");
    if n > 5 {
        out.push_str(&format!(" (+{} more)", n - 5));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// More than five targets: *which* five print must not follow log order —
    /// the invariant the property tests cannot reach (their scenes hold ≤4 rows).
    #[test]
    fn id_list_cut_is_order_independent() {
        let ids: Vec<String> = (0..6).map(|i| format!("01{:024}", i)).collect();
        let rows: Vec<core::Row> = ids
            .iter()
            .map(|id| {
                let mut r = core::Row::new("me", "issue", "t", vec![]);
                r.id = id.clone();
                r
            })
            .collect();
        let w = core::abbrev(&core::Log {
            rows,
            ..Default::default()
        });
        let mut rev = ids.clone();
        rev.reverse();
        assert_eq!(id_list(&ids, &w), id_list(&rev, &w));
    }
}
