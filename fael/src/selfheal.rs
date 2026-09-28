//! Self-heal on `add` (PLAN-fael-durable-log chunks 3b–e): fael answers from
//! the log before it asks the agent. (b) a repeated note on the same writer +
//! branch + files supersedes the open one itself; (c) a caller-supplied key is
//! the stronger identity — the single open row with the same kind + key +
//! writer supersedes too; (d) a `Supersedes <id>` the flag left off sets it,
//! and a flag that resolves to nothing is rescued by the text when the text
//! names exactly one open row; (e) the one key these files already carry
//! becomes the row's key. The Stop-hook debt pattern (a row every turn, nothing
//! closing the old one) can no longer pile up, and a row filed where a topic
//! already lives carries that topic's identity. CLI and MCP share
//! `write::add_row`, so both behave the same.
//! Every automatic choice is reported in one info line — info, not a warning,
//! so it never counts as an ask. When several rows match or the rules
//! disagree, the row is filed, nothing is guessed, and one info line names
//! what was left open — never a reject, which would have no way out and fires
//! every Stop-hook turn on a branch already in debt.

use crate::core;
use std::collections::BTreeSet;

/// What self-heal decided: the supersedes value core should resolve (a
/// caller's flag only when it resolves to nothing — (d) then replaces it)
/// plus info lines.
pub(crate) struct Heal {
    pub supersedes: Option<String>,
    pub notes: Vec<String>,
}

/// Fill an absent `--supersedes` from the open log: (d) the text that names a
/// row, then (c) same kind + key — the key is the identity of the topic, any
/// branch — then (b) notes on the same writer + branch with overlapping files.
/// Zero matches → nothing; exactly one of mine → supersede it and say so;
/// several, or one of another writer → file the row and list what was kept. A
/// caller-given flag passes through untouched unless it resolves to nothing,
/// in which case the text is the only place left to say what to supersede.
pub(crate) fn heal(
    log: &core::Log,
    st: &core::Stamp,
    row: &core::Row,
    flag: Option<&str>,
) -> Result<Heal, String> {
    let mut h = Heal {
        supersedes: flag.map(String::from),
        notes: vec![],
    };
    let open = open_rows(log);
    // ids print at their shortest unique prefix, same as render/doctor, so any
    // of them pastes straight into `--supersedes` — unique against the row
    // being added too, which a fast caller may write in the same millisecond
    let w = core::abbrev(log).with(&row.id);
    let short = |r: &core::Row| w.short(&r.id).to_string();

    if let Some(f) = flag {
        if core::resolve(log, f).is_ok() {
            return Ok(h);
        }
        // (d) broken-flag rescue: the caller asked to supersede something the
        // id does not name, so the text may — an id after a "supersede*" word,
        // exactly one open row, or the original reject stands (0 or several:
        // R5/R6). The word still leads the id (§6d): a text that only mentions
        // a row in passing must not close it, flag broken or not
        if let [one] = text_targets(log, &open, &row.text).as_slice() {
            h.supersedes = Some(one.id.clone());
            h.notes.push(format!(
                "--supersedes {f:?} matched nothing; used {} from the text",
                short(one)
            ));
        }
        return Ok(h);
    }
    // (d) the text says what this row supersedes but the flag was left off:
    // an id after a "supersede*" word, never an id merely mentioned in passing
    match text_targets(log, &open, &row.text).as_slice() {
        [one] => {
            let target = one.id.clone();
            h.supersedes = Some(target.clone());
            h.notes
                .push(format!("superseded {} (id in the text)", short(one)));
            if row.kind == "note" {
                list_also(&open, st, &row.files, &target, &w, &mut h.notes);
            }
            return Ok(h);
        }
        many @ [_, ..] => {
            h.notes.push(format!(
                "open rows {} are named in the text — kept all; pass --supersedes <id> to replace one",
                id_list(many, &w)
            ));
            return Ok(h);
        }
        [] => {}
    }
    // (c) the caller's key is the strongest identity — see `by_key`
    if let Some(h) = by_key(st, row, &open, &w) {
        return Ok(h);
    }
    // (b) files: notes only, same writer + branch, overlapping files
    if row.kind == "note" {
        match files_match(&open, st, &row.files).as_slice() {
            [one] => {
                h.supersedes = Some(one.id.clone());
                h.notes.push(match &st.branch {
                    Some(b) => format!(
                        "superseded {} (open note, same branch {b}, same files)",
                        short(one)
                    ),
                    None => format!("superseded {} (open note, same files)", short(one)),
                });
            }
            many @ [_, ..] => h.notes.push(format!(
                "open notes {} overlap these files — kept all; pass --supersedes <id> to replace one",
                id_list(many, &w)
            )),
            [] => {}
        }
    }
    Ok(h)
}

/// (c) The caller's key is the strongest identity: same kind + key, any
/// branch, but only my own row is mine to close. `None` — no key on the row,
/// or no open row carrying that kind + key — lets `heal` fall through to (b).
/// Auto-key (e) never reaches here: it runs after `heal`, so a key the caller
/// did not write can't close a row.
fn by_key(
    st: &core::Stamp,
    row: &core::Row,
    open: &[&core::Row],
    w: &core::Abbrev,
) -> Option<Heal> {
    let k = row.key.as_deref()?;
    let mut h = Heal {
        supersedes: None,
        notes: vec![],
    };
    let short = |r: &core::Row| w.short(&r.id).to_string();
    match key_hits(open, &row.kind, k).as_slice() {
        [one] if one.by == st.by => {
            let target = one.id.clone();
            h.supersedes = Some(target.clone());
            h.notes.push(format!(
                "superseded {} (open {}, same key {k})",
                short(one),
                row.kind
            ));
            if row.kind == "note" {
                list_also(open, st, &row.files, &target, w, &mut h.notes);
            }
            Some(h)
        }
        [one] => {
            h.notes.push(format!(
                "open {} {} uses key {k} (another writer) — kept open",
                row.kind,
                short(one)
            ));
            Some(h)
        }
        many @ [_, ..] => {
            h.notes.push(format!(
                "open rows {} already use key {k} — kept all; pass --supersedes <id> to replace one",
                id_list(many, w)
            ));
            Some(h)
        }
        [] => None,
    }
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

/// Open rows the text names after a "supersede*" word. The word still leads
/// the id in the broken-flag rescue — the caller asked to supersede something,
/// but a text that only mentions a row in passing ("see 01A… for context")
/// must not close it (§6d). Ids of rows already closed or superseded are not
/// targets either: naming them changes nothing.
fn text_targets<'a>(log: &'a core::Log, open: &[&'a core::Row], text: &str) -> Vec<&'a core::Row> {
    let mut out: Vec<&core::Row> = vec![];
    let mut armed = false;
    for tok in text.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric());
        if t.is_empty() {
            continue;
        }
        if !armed {
            armed = t.to_ascii_lowercase().starts_with("supersede");
            continue;
        }
        // resolution is exact-id or unique prefix, same as --supersedes, so a
        // word that merely looks like an id resolves to nothing and is skipped
        let Ok(r) = core::resolve(log, t) else {
            continue;
        };
        if open.iter().any(|o| o.id == r.id) && !out.iter().any(|o| o.id == r.id) {
            out.push(r);
        }
    }
    out
}

/// The open notes besides `target` that overlap these files — named so a note
/// some other rule kept open never hides behind the row that was superseded.
fn list_also(
    open: &[&core::Row],
    st: &core::Stamp,
    files: &[String],
    target: &str,
    w: &core::Abbrev,
    out: &mut Vec<String>,
) {
    let also: Vec<&core::Row> = files_match(open, st, files)
        .into_iter()
        .filter(|r| r.id != target)
        .collect();
    if !also.is_empty() {
        out.push(format!(
            "note {} also overlaps these files — kept open",
            id_list(&also, w)
        ));
    }
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

/// Up to 5 ids shortened by `w` (`core::abbrev`), then `(+N more)`. Never a
/// fixed `[..8]`: a ULID's leading characters are its millisecond timestamp, so
/// rows written in the same second share them and `--supersedes <prefix>`
/// would be ambiguous — `Abbrev::short` lengthens a prefix until it is unique.
fn id_list(rows: &[&core::Row], w: &core::Abbrev) -> String {
    let mut s: Vec<&str> = rows.iter().take(5).map(|r| w.short(&r.id)).collect();
    s.sort_unstable();
    let mut out = s.join(", ");
    if rows.len() > 5 {
        out.push_str(&format!(" (+{} more)", rows.len() - 5));
    }
    out
}
