//! The done gate e2 rides on: linked ledger issues must be closed before a chunk
//! finishes, and open unlinked issues on the touched files are a warn. Crew knows
//! the linked ids (crew.db `chunk_issue`) but not their state, so it asks here
//! through `fael held done` — no new command or flag, only this stdin/output
//! contract. Stdin is the files one per line (as before), then when crew links
//! issues a `---issues---` line and one linked id per line after it.

use crate::core;

/// The stdin separator between touched files and linked issue ids.
pub(crate) const LINKED_MARKER: &str = "---issues---";
/// The stdin separator before the chunk's declared scope lines (done only: start's
/// files already are the scope).
pub(crate) const SCOPE_MARKER: &str = "---scope---";

/// Parsed `fael held` stdin.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Input {
    pub files: Vec<String>,
    pub scope: Vec<String>,
    pub linked: Vec<String>,
}

/// Split raw stdin into files, scope and linked issue ids. No marker = old
/// callers: every line is a file, no scope, no linked check.
pub(crate) fn parse_stdin(raw: &str) -> Input {
    let mut inp = Input::default();
    let mut section = 0; // 0 files, 1 scope, 2 linked
    for line in raw.lines() {
        match line.trim() {
            SCOPE_MARKER => section = 1,
            LINKED_MARKER => section = 2,
            "" => {}
            _ if section == 0 => inp.files.push(line.to_string()),
            tok => [&mut inp.files, &mut inp.scope, &mut inp.linked][section].push(tok.to_string()),
        }
    }
    inp
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

/// Open unlinked issues naming a file of `files`, whenever filed — an older one
/// on the same files is as unfinished as one filed this run. Ledger order, so the
/// warn reads oldest first.
pub(crate) fn unlinked_open<'a>(
    log: &'a core::Log,
    files: &[String],
    linked: &[String],
) -> Vec<&'a core::Row> {
    if files.iter().all(|f| f.trim().is_empty()) {
        return vec![];
    }
    log.rows
        .iter()
        .filter(|r| r.kind == "issue" && open(log, &r.id))
        .filter(|r| !linked.contains(&r.id))
        .filter(|r| core::overlap(&r.files, files))
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

/// Warn text for open unlinked issues on the touched files and the chunk's scope.
/// Empty when none — a hint, never a refusal.
pub(crate) fn warn_text(log: &core::Log, files: &[String], linked: &[String]) -> String {
    let found = unlinked_open(log, files, linked);
    let mut s = String::new();
    for r in found.iter().take(START_ISSUES_MAX) {
        s.push_str(&format!(
            "warn: open issue {} \"{}\" on {} not linked to this chunk — fael find {}\n",
            r.id,
            r.title.clone().unwrap_or_else(|| r.display_title()),
            r.files.join(","),
            r.id
        ));
    }
    if found.len() > START_ISSUES_MAX {
        s.push_str(&format!(
            "+{} more: fael find --files {} --kind issue\n",
            found.len() - START_ISSUES_MAX,
            files.join(",")
        ));
    }
    s
}

/// Most issues `chunk start` and `chunk done` name; the rest ride a count and the find line.
const START_ISSUES_MAX: usize = 5;

/// `chunk start`: the open issues on files under the scope that the chunk does not
/// link, newest first, each with the call that links it; `{uid}` is the caller's
/// to fill. Empty when none. A fact list — fael never says an issue is the
/// chunk's to fix.
pub(crate) fn start_text(log: &core::Log, scope: &[String], linked: &[String]) -> String {
    let found = unlinked_open(log, scope, linked);
    if found.is_empty() {
        return String::new();
    }
    let mut s = String::from("\nopen issues on this scope (not linked):\n");
    for r in found.iter().rev().take(START_ISSUES_MAX) {
        s.push_str(&format!(
            "- {} \"{}\" — link: fael chunk edit {{uid}} --issue +{}\n",
            r.id,
            r.title.clone().unwrap_or_else(|| r.display_title()),
            r.id
        ));
    }
    if found.len() > START_ISSUES_MAX {
        s.push_str(&format!(
            "+{} more: fael find --files {} --kind issue\n",
            found.len() - START_ISSUES_MAX,
            scope.join(",")
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
        let inp = parse_stdin("src/a.rs\nsrc/b.rs");
        assert_eq!(inp.files, vec!["src/a.rs", "src/b.rs"]);
        assert!(inp.linked.is_empty() && inp.scope.is_empty());
    }

    #[test]
    fn stdin_with_marker_splits_linked_ids() {
        let inp = parse_stdin("src/a.rs\n---scope---\nsrc/\n---issues---\n01AAA\n01BBB\n");
        assert_eq!(inp.files, vec!["src/a.rs"]);
        assert_eq!(inp.scope, vec!["src/"]);
        assert_eq!(inp.linked, vec!["01AAA", "01BBB"]);
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
        let warn = warn_text(&log, &files, std::slice::from_ref(&linked.id));
        assert!(warn.contains(&fresh.id), "{warn}");
        assert!(warn.contains("fael find"), "{warn}");
        assert!(!warn.contains(&linked.id), "linked stays out: {warn}");
        assert!(!warn.contains(&other.id), "other files stay out: {warn}");
        assert!(warn_text(&log, &[], std::slice::from_ref(&linked.id)).is_empty());
    }

    #[test]
    fn start_text_names_newest_five_unlinked_then_a_count() {
        let rows: Vec<Row> = (0..7)
            .map(|i| issue(&format!("01ISS{i:023}"), "w/a.rs"))
            .collect();
        let linked = rows[6].id.clone();
        let log = Log {
            rows,
            ..Log::default()
        };
        let t = start_text(&log, &["w/".to_string()], std::slice::from_ref(&linked));
        assert!(t.contains("open issues on this scope"), "{t}");
        assert!(t.contains("--issue +01ISS00000000000000000000005"), "{t}");
        assert!(!t.contains(&linked), "linked stays out: {t}");
        assert!(
            !t.contains("01ISS00000000000000000000000"),
            "oldest cut: {t}"
        );
        assert!(
            t.contains("+1 more: fael find --files w/ --kind issue"),
            "{t}"
        );
        assert!(start_text(&log, &["x/".to_string()], &[]).is_empty());
    }
}
