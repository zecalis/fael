//! `chunk add|edit --scope`: each path checked against the repo once written
//! (issue 01M4Q4HHX) — a typo'd scope silently weakens every overlap check, so it is said;
//! never refused, a chunk may create the file.

use fael_crew::{Store, overlap};
use std::collections::HashSet;
use std::path::Path;

use super::chunk_scope_gone as gone;
use super::chunk_scope_skip as skip;

/// A warning on stderr for each scope path of chunk `key` that names nothing under the
/// repo: its directory is missing, or holds a name within edit distance 2. A new file or
/// directory under an existing one passes silently.
/// Then each path the stored brief names that exists under the repo but outside the scope,
/// and each id/key the brief names whose row is closed.
/// `at_start`: the agent starting the chunk reads it — it gets `chunk note`, never `chunk edit`
/// (which clears the owner's approval; issue 01M5QF1R).
pub(crate) fn warn(s: &Store, r: &crate::Repo, key: &str, at_start: bool) {
    let Ok(b) = s.show(key) else { return };
    let root = &r.root;
    brief_paths(s, root, &b, at_start);
    for p in b.scope.as_deref().unwrap_or_default().lines() {
        if let Some(why) = missing(root, p) {
            let ask = if at_start {
                "go on; tell the owner in the done text".to_string()
            } else {
                format!("a typo? fael chunk edit {key} --scope …")
            };
            eprintln!("warning: scope {p:?} {why} in {} — {ask}", root.display());
        } else if let Some(w) = gone::warning(root, &b.uid, p, at_start) {
            eprintln!("{w}");
        }
    }
    let log = crate::read(r);
    for w in stale_warnings(&b.title, &b.brief, &b.issues, &log, &b.uid) {
        eprintln!("{w}");
    }
    for w in super::chunk_scope_report::warnings(&b.title, &b.brief, &log) {
        eprintln!("{w}");
    }
}

/// Linked issues plus brief cites whose row is closed: the board's `stale`.
/// A linked `--issue` id closed in the ledger is primary (done elsewhere);
/// a brief mention is secondary. Deduped by row id — a link already said
/// stays silent in the brief scan.
pub(crate) fn stale_warnings(
    title: &str,
    brief: &str,
    issues: &[String],
    log: &fael_core::Log,
    uid: &str,
) -> Vec<String> {
    let shut = fael_core::closed(log);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for id in issues {
        if !shut.contains(id.as_str()) || !seen.insert(id.clone()) {
            continue;
        }
        out.push(linked_line(id, &close_first_line(log, id), uid));
    }
    for (cite, first, id) in closed_cites_ids(title, brief, log) {
        if !seen.insert(id) {
            continue;
        }
        out.push(closed_line(&cite, &first, uid));
    }
    out
}

/// Linked-issue warning: the fix landed elsewhere, so the action is drop
/// (not a brief edit — the link stays). A deliberate reopen starts with
/// `--force` after a note.
fn linked_line(id: &str, first: &str, uid: &str) -> String {
    format!(
        "warning: linked issue {id} — closed: {first} — done elsewhere? fael chunk drop {uid} …"
    )
}

/// `fael chunk start` (issue 01M5FMHQX): a chunk whose linked issue is
/// closed, or whose brief cites a closed row, is refused before anything is
/// claimed; the correction goes in a note, `--force` starts anyway.
pub(crate) fn gate(s: &Store, r: &crate::Repo, uid: &str) -> Result<(), String> {
    let b = s.show(uid)?;
    if s.chunk_place(&b.uid)?
        .is_some_and(|(st, ..)| st == "running")
    {
        return Ok(());
    }
    let log = crate::read(r);
    let shut = fael_core::closed(&log);
    let mut seen: HashSet<String> = HashSet::new();
    let mut lines: Vec<String> = Vec::new();
    for id in &b.issues {
        if !shut.contains(id.as_str()) || !seen.insert(id.clone()) {
            continue;
        }
        lines.push(format!(
            "  linked issue {id} — closed: {}",
            close_first_line(&log, id)
        ));
    }
    for (cite, first, id) in closed_cites_ids(&b.title, &b.brief, &log) {
        if !seen.insert(id) {
            continue;
        }
        lines.push(format!("  {cite} — closed: {first}"));
    }
    if lines.is_empty() {
        return Ok(());
    }
    Err(format!(
        "rejected: chunk {} cites closed rows:\n{}\nput the correction in `fael chunk note {} \"<text>\"` (never `chunk edit`: it clears the owner's approval); `--force` starts anyway",
        b.uid,
        lines.join("\n"),
        b.uid
    ))
}

/// `(cite, close text's first line, row id)` — the id lets the linked-issue
/// check dedup a brief cite of the same row.
fn closed_cites_ids(
    title: &str,
    brief: &str,
    log: &fael_core::Log,
) -> Vec<(String, String, String)> {
    let text = format!("{title} {brief}");
    let shut = fael_core::closed(log);
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for tok in fael_core::id_tokens(&text) {
        let fael_core::Ref::One(row) = fael_core::ref_state(log, tok) else {
            continue;
        };
        if !shut.contains(row.id.as_str()) || !seen.insert(row.id.as_str()) {
            continue;
        }
        out.push((
            tok.to_string(),
            close_first_line(log, &row.id),
            row.id.clone(),
        ));
    }
    for k in fael_core::keys(log, None) {
        if !fael_core::names_key(&text, &k.key) {
            continue;
        }
        let hits: Vec<&fael_core::Row> = log
            .rows
            .iter()
            .filter(|r| r.key.as_deref() == Some(k.key.as_str()))
            .collect();
        let [row] = hits.as_slice() else { continue };
        if !shut.contains(row.id.as_str()) || !seen.insert(row.id.as_str()) {
            continue;
        }
        out.push((
            k.key.clone(),
            close_first_line(log, &row.id),
            row.id.clone(),
        ));
    }
    out
}

/// One closed-ref warning: the close text's first line, so the planner sees why it settled.
fn closed_line(cite: &str, first: &str, uid: &str) -> String {
    format!("warning: brief cites {cite} — closed: {first} — fael chunk edit {uid} --brief …")
}

/// The text a row was closed with, first line: the newest close row wins, else the folded
/// `closed.text` a compacted row carries.
fn close_first_line(log: &fael_core::Log, id: &str) -> String {
    let from_close = log
        .closes
        .iter()
        .filter(|c| c.reference.as_deref() == Some(id))
        .max_by(|a, b| a.ts.cmp(&b.ts))
        .map(|c| c.text.clone());
    let from_row = || {
        log.rows.iter().find(|r| r.id == id).and_then(|r| {
            r.extra
                .get("closed")
                .and_then(|v| v.get("text"))
                .and_then(|t| t.as_str())
                .map(String::from)
        })
    };
    let text = from_close.or_else(from_row).unwrap_or_default();
    text.lines().next().unwrap_or("").trim().to_string()
}

fn missing(at: &Path, p: &str) -> Option<String> {
    let p = p.trim_end_matches('/');
    if p.is_empty() || at.join(p).exists() {
        return None;
    }
    let dir = p.rsplit_once('/').map_or("", |(d, _)| d);
    if !at.join(dir).is_dir() {
        return Some(format!("names nothing: no directory {dir:?}"));
    }
    fael_core::sibling_suggest(at, p).map(|near| format!("is not on disk — {near:?}?"))
}

/// The brief is the planner's text, never the rendered start. A warning only: the scope is
/// the planner's to change, and a path not on disk may be a file the chunk creates.
/// A path on a line that says "leave alone" never warns: the planner saw it and
/// deliberately scoped it out (a `drop?` brief naming what not to touch).
/// A path on a `Check:`/`Run:`/`Verify:` clause never warns either: the clause
/// names a command to run, not a file to edit; it only warns when git removed the path.
fn brief_paths(s: &Store, root: &Path, b: &fael_crew::Brief, at_start: bool) {
    let scope: Vec<String> = b
        .scope
        .as_deref()
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let (at, stale) = crate::board::clock();
    let others = s
        .board(&at[..10], &stale)
        .map(|b| b.chunks)
        .unwrap_or_default();
    for line in b.brief.lines() {
        if line.to_lowercase().contains("leave alone") {
            continue;
        }
        // a `Check:`/`Run:`/`Verify:` clause names commands to run, not files
        // to edit (issue 01M59BNT): `scripts/gate.sh` there must not widen the
        // scope. Clauses, not lines: a brief is often one paragraph with
        // `…bump). Check: …gate.sh green.` mid-line.
        for clause in clauses(line) {
            // on a check clause the removed/renamed word is the only one said
            if is_check_line(clause) {
                for p in paths(clause) {
                    if let Some(w) = gone::warning(root, &b.uid, &p, at_start) {
                        eprintln!("{w}");
                    }
                }
                continue;
            }
            for sentence in skip::sentences(clause) {
                for p in paths(sentence) {
                    if let Some(w) = gone::warning(root, &b.uid, &p, at_start) {
                        eprintln!("{w}");
                    }
                    let hit = [p.clone()];
                    // "do not touch X" / "run X" name a path without asking an edit
                    // (issue 01M5D5FCS)
                    if !root.join(&p).exists()
                        || overlap(&hit, &scope)
                        || skip::skipped(sentence, &p)
                    {
                        continue;
                    }
                    let holder = others.iter().find(|c| {
                        c.uid != b.uid
                            && !["done", "replaced", "dropped"].contains(&c.state.as_str())
                            && overlap(&hit, &c.scope)
                    });
                    eprintln!("{}", outside_line(&b.uid, &p, holder, at_start));
                }
            }
        }
    }
}

/// One brief-path warning. The planner may widen the scope; the starter may not, so it is
/// told to coordinate with the chunk holding the path (`chunk note`) or go on.
fn outside_line(
    uid: &str,
    p: &str,
    holder: Option<&fael_crew::BoardChunk>,
    at_start: bool,
) -> String {
    let label =
        |c: &fael_crew::BoardChunk| format!("{}/{}", c.plan, c.label.as_deref().unwrap_or(&c.uid));
    match (at_start, holder) {
        (true, Some(c)) => format!(
            "warning: brief names {p}, in {}'s scope ({}) — editing it? coordinate: fael chunk note {} \"<text>\" · only reading? go on",
            label(c),
            c.state,
            c.uid
        ),
        (true, None) => format!(
            "warning: brief names {p}, not in scope — go on; chunk done lists it as outside"
        ),
        (false, h) => format!(
            "warning: brief names {p}, not in scope{} — fael chunk edit {uid} --scope … · only read? write \"read {p}\"{}",
            h.map_or(String::new(), |c| format!(" (in {}'s scope)", label(c))),
            h.map_or(String::new(), |c| format!(
                " · or coordinate: fael chunk note {} \"<text>\"",
                c.uid
            ))
        ),
    }
}

/// A brief line split at sentence ends: a `Check:` clause mid-paragraph is still
/// a run mention. Paths hold no spaces, so no path breaks across a split.
fn clauses(line: &str) -> Vec<&str> {
    let mut out = vec![line];
    for sep in [". ", "; ", "? ", "! "] {
        out = out.iter().flat_map(|s| s.split(sep)).collect();
    }
    out
}

/// A verification line names commands to run, not files to edit: `Check:`,
/// `Run:`, `Verify:` (case-insensitive, after any `>`, `-`, `*` quote).
fn is_check_line(line: &str) -> bool {
    let t = line
        .trim_start()
        .trim_start_matches(['>', '-', '*', '#'])
        .trim_start()
        .to_lowercase();
    ["check:", "run:", "verify:"]
        .iter()
        .any(|p| t.starts_with(p))
}

/// Path-like tokens of `text` (a `/` and a dot, or a trailing `/`), once each, with the
/// backticks, quotes, trailing punctuation and `:line` stripped.
fn paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in text.split_whitespace() {
        let end = |c: char| "`\"'()[]<>,;.:!?".contains(c);
        let mut t = t
            .trim_start_matches(|c| "`\"'([<".contains(c))
            .trim_end_matches(end);
        while let Some((head, line)) = t.rsplit_once(':') {
            if !line.chars().all(|c| c.is_ascii_digit()) {
                break;
            }
            t = head;
        }
        let t = t.trim_end_matches(end);
        let ok = t.contains('/')
            && (t.ends_with('/') || t.contains('.'))
            && !t.starts_with('/')
            && !t.contains("://")
            && !t.contains("..")
            && !t.contains('*');
        if ok && !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{clauses, is_check_line, stale_warnings};

    #[test]
    fn check_run_verify_lines_are_run_mentions_not_edits() {
        for l in [
            "Check: scripts/gate.sh green.",
            "check: update the shape lock, scripts/gate.sh green",
            "Run: cargo test -p fael-core",
            "Verify: fael stats --json shows flow",
            "> Check: scripts/gate.sh green",
            "- Check: gate green",
        ] {
            assert!(is_check_line(l), "{l}");
        }
        for l in [
            "Add one additive flow object in fael-core/src/stats/flow.rs.",
            "Edit fael-core/src/stats/aggregate.rs, do not grow it past 400 lines.",
            "wired through mod.rs and aggregate.rs",
        ] {
            assert!(!is_check_line(l), "{l}");
        }
    }

    #[test]
    fn a_mid_paragraph_check_clause_splits_off() {
        let c = clauses("Edit src/x.rs. Check: run e2e/y.ts suite, gate green.");
        assert_eq!(c.len(), 2);
        assert!(!is_check_line(c[0]));
        assert!(is_check_line(c[1]));
    }

    fn log_with_closed_and_open() -> fael_core::Log {
        let shut = fael_core::Row {
            id: "01M0000000000000000000001".into(),
            kind: "decision".into(),
            text: "cash basis".into(),
            key: Some("test:cash-basis-edges".into()),
            ..Default::default()
        };
        let open = fael_core::Row {
            id: "01M0000000000000000000002".into(),
            kind: "decision".into(),
            text: "deferred work stays open".into(),
            key: Some("test:open-topic".into()),
            ..Default::default()
        };
        let close = fael_core::Row {
            id: "01M0000000000000000000003".into(),
            reference: Some(shut.id.clone()),
            text: "shipped in v2\nmore detail".into(),
            ts: "2026-10-12T00:00:00Z".into(),
            ..Default::default()
        };
        fael_core::Log {
            rows: vec![shut, open],
            closes: vec![close],
            ..Default::default()
        }
    }

    #[test]
    fn a_closed_key_warns_and_an_open_key_stays_silent() {
        let log = log_with_closed_and_open();
        let uid = "01M00000000000000000000W1";
        let both = stale_warnings(
            "t",
            "Fixes test:cash-basis-edges, see test:open-topic",
            &[],
            &log,
            uid,
        );
        assert_eq!(both.len(), 1, "{both:?}");
        assert!(
            both[0].contains("brief cites test:cash-basis-edges")
                && both[0].contains("closed: shipped in v2")
                && both[0].contains(&format!("fael chunk edit {uid} --brief")),
            "{}",
            both[0]
        );
        // a missing key, an unknown id and a key shared by two rows stay silent
        let mut dup = log.clone();
        dup.rows.push(fael_core::Row {
            id: "01M0000000000000000000004".into(),
            kind: "decision".into(),
            text: "same key again".into(),
            key: Some("test:cash-basis-edges".into()),
            ..Default::default()
        });
        let silent = stale_warnings(
            "t",
            "test:cash-basis-edges test:no-such-key 01M0000000000000000000009",
            &[],
            &dup,
            uid,
        );
        assert!(silent.is_empty(), "{silent:?}");
        let open_only = stale_warnings("t", "see test:open-topic, deferred work", &[], &log, uid);
        assert!(open_only.is_empty(), "{open_only:?}");
        // an id token of the closed row warns too, with the close's first line only
        let by_id = stale_warnings("t", "see 01M0000000000000000000001", &[], &log, uid);
        assert_eq!(by_id.len(), 1, "{by_id:?}");
        assert!(
            by_id[0].contains("shipped in v2") && !by_id[0].contains("more detail"),
            "{}",
            by_id[0]
        );
    }

    #[test]
    fn a_closed_linked_issue_warns_and_dedups_its_brief_cite() {
        let log = log_with_closed_and_open();
        let uid = "01M00000000000000000000W1";
        let shut = "01M0000000000000000000001".to_string();
        // linked only: warns with a drop hint, not a brief-edit hint
        let linked = stale_warnings(
            "t",
            "unrelated work",
            std::slice::from_ref(&shut),
            &log,
            uid,
        );
        assert_eq!(linked.len(), 1, "{linked:?}");
        assert!(
            linked[0].contains("linked issue 01M0000000000000000000001")
                && linked[0].contains("closed: shipped in v2")
                && linked[0].contains(&format!("fael chunk drop {uid}")),
            "{}",
            linked[0]
        );
        // the same row cited in the brief too: said once
        let dup = stale_warnings("t", "see 01M0000000000000000000001", &[shut], &log, uid);
        assert_eq!(dup.len(), 1, "{dup:?}");
        // an open linked issue stays silent
        let open = stale_warnings(
            "t",
            "unrelated work",
            &["01M0000000000000000000002".to_string()],
            &log,
            uid,
        );
        assert!(open.is_empty(), "{open:?}");
    }
}
