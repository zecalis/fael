//! What `chunk start` prints under the stored brief (issue cites-truth): one line per
//! ledger row the title or brief cites, with its live state — open, closed (plus the
//! close text's first line), or superseded (plus its successor). A row carrying `cmd:`
//! and `result:` lines adds `fael verify <id>`. Facts only, never a refusal.

use std::collections::HashSet;

/// One cited row's line, without the leading `- `: `<id> — <state> — <title>[ · fael verify <id>]`.
pub(crate) fn cited_lines(title: &str, brief: &str, log: &fael_core::Log) -> Vec<String> {
    let shut = fael_core::closed(log);
    let gone = fael_core::superseded(log);
    let succ = fael_core::successors(log);
    let keys = fael_core::keys(log, None);
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for tok in fael_core::id_tokens(&format!("{title} {brief}")) {
        let fael_core::Ref::One(row) = fael_core::ref_state(log, tok) else {
            continue;
        };
        if !seen.insert(row.id.as_str()) {
            continue;
        }
        out.push(line(row, &shut, &gone, &succ, log));
    }
    for k in &keys {
        if !fael_core::names_key(&format!("{title} {brief}"), &k.key) {
            continue;
        }
        let hits: Vec<&fael_core::Row> = log
            .rows
            .iter()
            .filter(|r| r.key.as_deref() == Some(k.key.as_str()))
            .collect();
        let [row] = hits.as_slice() else { continue };
        if !seen.insert(row.id.as_str()) {
            continue;
        }
        out.push(line(row, &shut, &gone, &succ, log));
    }
    out
}

fn line(
    row: &fael_core::Row,
    shut: &HashSet<&str>,
    gone: &HashSet<&str>,
    succ: &std::collections::HashMap<&str, &str>,
    log: &fael_core::Log,
) -> String {
    let title = row.display_title();
    let mut s = if gone.contains(row.id.as_str()) {
        let to = succ.get(row.id.as_str()).copied().unwrap_or("?");
        format!("{} — superseded → {to} — {title}", row.id)
    } else if shut.contains(row.id.as_str()) {
        let first = crate::chunk_scope::close_first_line(log, &row.id);
        if first.is_empty() {
            format!("{} — closed — {title}", row.id)
        } else {
            format!("{} — closed: {first} — {title}", row.id)
        }
    } else {
        format!("{} — open — {title}", row.id)
    };
    if has_verify(row) {
        s.push_str(&format!(" · fael verify {}", row.id));
    }
    s
}

/// A row `fael verify` can re-run: its text carries both a `cmd:` and a `result:` line.
fn has_verify(row: &fael_core::Row) -> bool {
    let mut cmd = false;
    let mut result = false;
    for l in row.text.lines() {
        let t = l.trim();
        if t.strip_prefix("cmd:").is_some_and(|v| !v.trim().is_empty()) {
            cmd = true;
        }
        if t.strip_prefix("result:")
            .is_some_and(|v| !v.trim().is_empty())
        {
            result = true;
        }
    }
    cmd && result
}

#[cfg(test)]
mod tests {
    use super::cited_lines;

    fn row(id: &str, text: &str) -> fael_core::Row {
        fael_core::Row {
            id: id.into(),
            kind: "decision".into(),
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn open_closed_and_superseded_lines() {
        let open = row("01M0000000000000000000001", "keep this");
        let shut = row("01M0000000000000000000002", "was broken");
        let old = row("01M0000000000000000000003", "old way");
        let mut next = row("01M0000000000000000000004", "new way");
        next.supersedes = Some(old.id.clone());
        let mut close = fael_core::Row::new("x", "note", "was broken → fixed", vec![]);
        close.reference = Some(shut.id.clone());
        let log = fael_core::Log {
            rows: vec![open, shut, old, next],
            closes: vec![close],
            ..Default::default()
        };
        let lines = cited_lines(
            "t",
            "see 01M0000000000000000000001 01M0000000000000000000002 01M0000000000000000000003",
            &log,
        );
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].contains("— open —"), "{}", lines[0]);
        assert!(
            lines[1].contains("— closed: was broken → fixed"),
            "{}",
            lines[1]
        );
        assert!(
            lines[2].contains("— superseded → 01M0000000000000000000004"),
            "{}",
            lines[2]
        );
    }

    #[test]
    fn a_cmd_result_row_adds_verify() {
        let r = row("01M0000000000000000000001", "hit\ncmd: echo 1\nresult: 1");
        let log = fael_core::Log {
            rows: vec![r],
            ..Default::default()
        };
        let lines = cited_lines("t", "see 01M0000000000000000000001", &log);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("fael verify 01M0000000000000000000001"),
            "{}",
            lines[0]
        );
    }
}
