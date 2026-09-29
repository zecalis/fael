//! Renderer: Verdict → Heal (chunk 1 of PLAN-fael-selfheal-verdict).
//!
//! Every string below is byte-identical to what the if-chain printed —
//! chunk 1 separates the words from the decision without changing one byte.
//! Chunk 3 may add ActWarn lines here; the Verdict carries what to print.

use super::decide::Verdict;
use crate::core;

/// What self-heal decided: the supersedes value core should resolve (a
/// caller's flag only when it resolves to nothing — (d) then replaces it)
/// plus info lines.
pub(crate) struct Heal {
    pub supersedes: Option<String>,
    pub notes: Vec<String>,
}

pub(crate) fn render(
    v: &Verdict,
    row: &core::Row,
    st: &core::Stamp,
    w: &core::Abbrev,
    flag: Option<&str>,
) -> Heal {
    let short = |id: &str| w.short(id).to_string();
    let k = row.key.as_deref().unwrap_or_default();
    match v {
        Verdict::FlagPassthrough | Verdict::FlagUnresolved => Heal {
            supersedes: flag.map(String::from),
            notes: vec![],
        },
        Verdict::FlagRescued { target } => Heal {
            supersedes: Some(target.clone()),
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
                notes,
            }
        }
        Verdict::TextHold { targets } => Heal {
            supersedes: None,
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
                notes,
            }
        }
        Verdict::KeyIssueKept { targets } => Heal {
            supersedes: None,
            notes: vec![format!(
                "open issue {} uses key {k} — kept open (a different finding)",
                id_list(targets, w)
            )],
        },
        Verdict::KeyOtherWriter { target } => Heal {
            supersedes: None,
            notes: vec![format!(
                "open {} {} uses key {k} (another writer) — kept open",
                row.kind,
                short(target)
            )],
        },
        Verdict::KeyMany { targets } => Heal {
            supersedes: None,
            notes: vec![format!(
                "open rows {} already use key {k} — kept all; pass --supersedes <id> to replace one",
                id_list(targets, w)
            )],
        },
        Verdict::FilesAct { target } => Heal {
            supersedes: Some(target.clone()),
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
            notes: vec![format!(
                "open notes {} overlap these files — kept all; pass --supersedes <id> to replace one",
                id_list(targets, w)
            )],
        },
        Verdict::Noop => Heal {
            supersedes: None,
            notes: vec![],
        },
    }
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
fn id_list(ids: &[String], w: &core::Abbrev) -> String {
    let mut s: Vec<&str> = ids.iter().take(5).map(|id| w.short(id)).collect();
    s.sort_unstable();
    let mut out = s.join(", ");
    if ids.len() > 5 {
        out.push_str(&format!(" (+{} more)", ids.len() - 5));
    }
    out
}
