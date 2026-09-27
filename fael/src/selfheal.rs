//! Self-heal on `add` (PLAN-fael-durable-log chunks 3b–c): a repeated note on
//! the same writer + branch + files supersedes the open one itself, and a
//! caller-supplied key is the stronger identity — the single open row with the
//! same kind + key + writer supersedes too. Either way the Stop-hook debt
//! pattern (a row every turn, nothing closing the old one) can no longer pile
//! up. CLI and MCP share `write::add_row`, so both behave the same.
//! The automatic choice is reported in one info line (`superseded <id>`) —
//! info, not a warning, so it never counts as an ask. When several rows match
//! or the rules disagree, the row is filed, nothing is guessed, and one info
//! line names what was left open — never a reject, which would have no way out
//! and fires every Stop-hook turn on a branch already in debt.
//! (d) text id and (e) auto-key follow in later commits.

use crate::core;

/// What self-heal decided: the supersedes value core should resolve (the
/// caller's flag untouched — (b)/(c) only fill an absent one) plus info lines.
pub(crate) struct Heal {
    pub supersedes: Option<String>,
    pub notes: Vec<String>,
}

/// Fill an absent `--supersedes` from the open log: (c) same kind + key first —
/// the key is the identity of the topic, any branch — then (b) notes on the
/// same writer + branch with overlapping files. Zero matches → nothing;
/// exactly one of mine → supersede it and say so; several, or one of another
/// writer → file the row and list what was kept. A caller-given flag always
/// passes through untouched.
pub(crate) fn heal(
    log: &core::Log,
    st: &core::Stamp,
    kind: &str,
    files: &[String],
    key: Option<&str>,
    flag: Option<&str>,
) -> Result<Heal, String> {
    let mut h = Heal {
        supersedes: flag.map(String::from),
        notes: vec![],
    };
    if h.supersedes.is_some() {
        return Ok(h);
    }
    let open = open_rows(log);
    // (c) the caller's key is the strongest identity: same kind + key, any
    // branch, but only my own row is mine to close. Auto-key (e) must never
    // reach here — guessing a key and then using the guess to close rows is
    // guessing twice.
    if let Some(k) = key {
        match key_hits(&open, kind, k).as_slice() {
            [one] if one.by == st.by => {
                h.supersedes = Some(one.id.clone());
                h.notes
                    .push(format!("superseded {} (open {kind}, same key {k})", one.id));
                // (b) may still see other notes: name them so nothing hides
                if kind == "note" {
                    let also: Vec<&core::Row> = files_match(&open, st, files)
                        .into_iter()
                        .filter(|r| r.id != one.id)
                        .collect();
                    if !also.is_empty() {
                        h.notes.push(format!(
                            "note {} also overlaps these files — kept open",
                            id_list(&also)
                        ));
                    }
                }
                return Ok(h);
            }
            [one] => {
                h.notes.push(format!(
                    "open {kind} {} uses key {k} (another writer) — kept open",
                    one.id
                ));
                return Ok(h);
            }
            many @ [_, ..] => {
                h.notes.push(format!(
                    "open rows {} already use key {k} — kept all; pass --supersedes <id> to replace one",
                    id_list(many)
                ));
                return Ok(h);
            }
            [] => {}
        }
    }
    // (b) files: notes only, same writer + branch, overlapping files
    if kind == "note" {
        match files_match(&open, st, files).as_slice() {
            [one] => {
                h.supersedes = Some(one.id.clone());
                h.notes.push(match &st.branch {
                    Some(b) => format!(
                        "superseded {} (open note, same branch {b}, same files)",
                        one.id
                    ),
                    None => format!("superseded {} (open note, same files)", one.id),
                });
            }
            many @ [_, ..] => h.notes.push(format!(
                "open notes {} overlap these files — kept all; pass --supersedes <id> to replace one",
                id_list(many)
            )),
            [] => {}
        }
    }
    Ok(h)
}

/// Open rows with this kind + key, any writer, any branch — chunk 3c. The
/// call site checks that only my own single row is superseded.
fn key_hits<'a>(open: &[&'a core::Row], kind: &str, key: &str) -> Vec<&'a core::Row> {
    open.iter()
        .copied()
        .filter(|r| r.kind == kind && r.key.as_deref() == Some(key))
        .collect()
}

/// Open notes, same writer + branch, overlapping files — chunk 3b.
fn files_match<'a>(
    open: &[&'a core::Row],
    st: &core::Stamp,
    files: &[String],
) -> Vec<&'a core::Row> {
    open.iter()
        .copied()
        .filter(|r| {
            r.kind == "note"
                && r.by == st.by
                && r.branch() == st.branch.as_deref()
                && r.files.iter().any(|f| files.contains(f))
        })
        .collect()
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
