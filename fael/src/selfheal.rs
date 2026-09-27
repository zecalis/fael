//! Self-heal on `add` (PLAN-fael-durable-log chunk 3b): a repeated note on
//! the same writer + branch + files supersedes the open one itself instead of
//! piling note debt — the Stop-hook pattern (a row every turn, nothing closing
//! the old one). CLI and MCP share `write::add_row`, so both behave the same.
//! The automatic choice is reported in one info line (`superseded <id>`) —
//! info, not a warning, so it never counts as an ask. Several open notes is
//! ambiguous: the row is filed, nothing superseded, one info line names the
//! candidates — never a guess, never a reject.
//! (c) key, (d) text id and (e) auto-key follow in later commits.

use crate::core;

/// What self-heal decided: the supersedes value core should resolve (the
/// caller's flag untouched — (b) only fills an absent one) plus info lines.
pub(crate) struct Heal {
    pub supersedes: Option<String>,
    pub notes: Vec<String>,
}

/// One open note, same writer + branch, overlapping files → supersede it.
/// Zero → nothing; several → file it, supersede nothing, list them. A caller-given `--supersedes`
/// always passes through untouched.
pub(crate) fn heal(
    log: &core::Log,
    st: &core::Stamp,
    kind: &str,
    files: &[String],
    flag: Option<&str>,
) -> Result<Heal, String> {
    let mut h = Heal {
        supersedes: flag.map(String::from),
        notes: vec![],
    };
    if h.supersedes.is_none() && kind == "note" {
        let cand: Vec<&core::Row> = open_rows(log)
            .into_iter()
            .filter(|r| {
                r.kind == "note"
                    && r.by == st.by
                    && r.branch() == st.branch.as_deref()
                    && r.files.iter().any(|f| files.contains(f))
            })
            .collect();
        match cand.as_slice() {
            [one] => {
                h.notes.push(match &st.branch {
                    Some(b) => {
                        format!(
                            "superseded {} (open note, same branch {b}, same files)",
                            one.id
                        )
                    }
                    None => format!("superseded {} (open note, same files)", one.id),
                });
                h.supersedes = Some(one.id.clone());
            }
            [] => {}
            // ponytail: several is ambiguous, but a reject here has no way out
            // (no flag files a distinct note) and fires every Stop-hook turn on
            // a branch already in debt — file it, supersede nothing, say so;
            // doctor ([Shipped]) owns the cleanup (§1 step 2)
            many => h.notes.push(format!(
                "open notes {} overlap these files — kept all; pass --supersedes <id> to replace one",
                id_list(many)
            )),
        }
    }
    Ok(h)
}

/// Open rows: neither closed nor superseded. Self-heal only ever touches
/// these — history stays history.
fn open_rows(log: &core::Log) -> Vec<&core::Row> {
    let closed = core::closed(log);
    let supd = core::superseded(log);
    log.rows
        .iter()
        .filter(|r| !closed.contains(r.id.as_str()) && !supd.contains(r.id.as_str()))
        .collect()
}

/// Up to 5 full ids, then `(+N more)`. Full ids on purpose: a ULID's leading
/// characters are its millisecond timestamp, so short prefixes collide for any
/// two rows written in the same second and `--supersedes <prefix>` would be
/// ambiguous — exactly what this line exists to avoid.
fn id_list(rows: &[&core::Row]) -> String {
    let mut s: Vec<&str> = rows.iter().take(5).map(|r| r.id.as_str()).collect();
    s.sort_unstable();
    let mut out = s.join(", ");
    if rows.len() > 5 {
        out.push_str(&format!(" (+{} more)", rows.len() - 5));
    }
    out
}
