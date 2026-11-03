//! The done gate e2 rides on: linked ledger issues must be closed before a chunk
//! finishes, and open unlinked issues on the touched files are a warn. Crew knows
//! the linked ids (crew.db `chunk_issue`) but not their state, so it asks here
//! through `fael held done` — no new command or flag, only this stdin/output
//! contract. Stdin is the files one per line (as before), then when crew links
//! issues a `---issues---` line and one linked id per line after it.

use crate::core;

/// The stdin separator between touched files and linked issue ids.
pub(crate) const LINKED_MARKER: &str = "---issues---";

/// Split raw stdin into (files, linked issue ids). No marker = old callers:
///
/// every line is a file, no linked check.
pub(crate) fn parse_stdin(raw: &str) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut linked = Vec::new();
    let mut after = false;
    for line in raw.lines() {
        if line.trim() == LINKED_MARKER {
            after = true;
            continue;
        }
        let target = if after { &mut linked } else { &mut files };
        let tok = line.trim();
        if after {
            if !tok.is_empty() {
                target.push(tok.to_string());
            }
        } else {
            target.push(line.to_string());
        }
    }
    (files, linked)
}

/// True for a row neither closed nor superseded.
fn open(log: &core::Log, id: &str) -> bool {
    !core::closed(log).contains(id) && !core::superseded(log).contains(id)
}

/// The linked ids still open as ledger issues, in link order. Unresolvable or
/// non-issue rows are not open (crew validated kind at link time).
pub(crate) fn linked_open<'a>(log: &'a core::Log, linked: &[String]) -> Vec<&'a core::Row> {
    let mut out = Vec::new();
    for id in linked {
        let row = log
            .rows
            .iter()
            .find(|r| r.id == *id)
            .map_or_else(|| core::resolve_row(log, id).ok(), Some);
        if let Some(r) = row
            && r.kind == "issue"
            && open(log, &r.id)
            && !out.iter().any(|o: &&core::Row| o.id == r.id)
        {
            out.push(r);
        }
    }
    out
}

/// Open unlinked issues naming a file of `files` and filed at or after
/// `since` (None = no start recorded, so every open unlinked one). Ledger
/// order, so the warn reads oldest first.
pub(crate) fn unlinked_open<'a>(
    log: &'a core::Log,
    files: &[String],
    linked: &[String],
    since: Option<i64>,
) -> Vec<&'a core::Row> {
    if files.iter().all(|f| f.trim().is_empty()) {
        return vec![];
    }
    let linked_ids: Vec<&str> = linked.iter().map(String::as_str).collect();
    log.rows
        .iter()
        .filter(|r| r.kind == "issue" && open(log, &r.id))
        .filter(|r| !linked_ids.iter().any(|l| r.id == **l))
        .filter(|r| core::overlap(&r.files, files))
        .filter(|r| since.is_none_or(|t| core::ts_ms(&r.ts).is_none_or(|ms| ms >= t)))
        .collect()
}

/// Refuse text when linked issues are still open, with one copyable
/// `fael close <id> "..."` per row. None = the gate passes.
pub(crate) fn gate_error(log: &core::Log, linked: &[String]) -> Option<String> {
    let open = linked_open(log, linked);
    if open.is_empty() {
        return None;
    }
    let mut s = String::from("linked issues still open — close each before done:\n");
    for r in open {
        s.push_str(&format!(
            "open linked issue {} \"{}\" — close it first: fael close {} \"...\"\n",
            r.id,
            r.title.clone().unwrap_or_else(|| r.display_title()),
            r.id
        ));
    }
    Some(s)
}

/// Warn text for open unlinked issues on the touched files. Empty when none —
/// a hint, never a refusal.
pub(crate) fn warn_text(
    log: &core::Log,
    files: &[String],
    linked: &[String],
    since: Option<i64>,
) -> String {
    let open = unlinked_open(log, files, linked, since);
    if open.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    for r in open {
        let files = r.files.join(",");
        s.push_str(&format!(
            "warn: open issue {} \"{}\" on {files} filed after the run started — fael find {}\n",
            r.id,
            r.title.clone().unwrap_or_else(|| r.display_title()),
            r.id
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Log, Row};

    fn issue(id: &str, file: &str) -> Row {
        Row {
            id: id.into(),
            kind: "issue".into(),
            files: vec![file.into()],
            title: Some(format!("title {id}")),
            ts: "2026-11-01T00:00:00Z".into(),
            ..Row::default()
        }
    }

    #[test]
    fn stdin_without_marker_is_files_only() {
        let (files, linked) = parse_stdin("src/a.rs\nsrc/b.rs");
        assert_eq!(files, vec!["src/a.rs", "src/b.rs"]);
        assert!(linked.is_empty());
    }

    #[test]
    fn stdin_with_marker_splits_linked_ids() {
        let (files, linked) = parse_stdin("src/a.rs\n---issues---\n01AAA\n01BBB\n");
        assert_eq!(files, vec!["src/a.rs"]);
        assert_eq!(linked, vec!["01AAA", "01BBB"]);
    }

    #[test]
    fn linked_open_names_only_open_issues_with_a_close_command() {
        let open = issue("01OPEN0000000000000000000001", "src/a.rs");
        let shut = issue("01SHUT0000000000000000000001", "src/a.rs");
        let log = Log {
            rows: vec![open.clone(), shut.clone()],
            closes: vec![Row::close("t", &shut.id, "done")],
            ..Log::default()
        };
        let ids = vec![open.id.clone(), shut.id.clone(), "01MISSING".into()];
        let found = linked_open(&log, &ids);
        assert_eq!(found, vec![&open]);
        let err = gate_error(&log, &ids).unwrap();
        assert!(
            err.contains(&format!("fael close {} \"...\"", open.id)),
            "{err}"
        );
        assert!(!err.contains(&shut.id), "{err}");
        assert!(gate_error(&log, std::slice::from_ref(&shut.id)).is_none());
        assert!(gate_error(&log, &[]).is_none());
    }

    #[test]
    fn unlinked_warn_lists_open_unlinked_on_the_files_only() {
        let linked = issue("01LINK0000000000000000000001", "src/a.rs");
        let fresh = issue("01FRESH000000000000000000001", "src/a.rs");
        let other = issue("01OTHER000000000000000000001", "src/b.rs");
        let log = Log {
            rows: vec![linked.clone(), fresh.clone(), other.clone()],
            ..Log::default()
        };
        let files = vec!["src/a.rs".to_string()];
        let warn = warn_text(&log, &files, std::slice::from_ref(&linked.id), None);
        assert!(warn.contains(&fresh.id), "{warn}");
        assert!(warn.contains("fael find"), "{warn}");
        assert!(!warn.contains(&linked.id), "linked stays out: {warn}");
        assert!(!warn.contains(&other.id), "other files stay out: {warn}");
        assert!(warn_text(&log, &[], std::slice::from_ref(&linked.id), None).is_empty());
    }
}
