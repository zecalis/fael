//! render · token estimates — one markdown line per row under a budget.

use super::{log, row};
use fael_core::*;

#[test]
fn render_cuts_at_budget_but_shows_one_row() {
    let l = log();
    let rows = find(&l, &Filter::default());
    let out = render(&l, &rows, 1);
    assert_eq!(out.lines().count(), 2, "{out}");
    // ranked first is the open issue 13 (urgent, to, kind all beat newest-id)
    assert!(out.starts_with(
        "- [A0000000000000000000000013] issue text of A0000000000000000000000013 → .\\src\\c.rs\n"
    ));
    assert!(out.ends_with("… +3 more over the 1-token budget — narrow the filter\n"));
    let all = Filter {
        all: true,
        ..Filter::default()
    };
    let out = render(&l, &find(&l, &all), 10_000);
    assert!(
        out.contains(
            "- [A0000000000000000000000011] decision (superseded → A0000000000000000000000014) text"
        ),
        "{out}"
    );
    assert!(out.contains("issue (closed) #auth:session"), "{out}");
}

/// A closed issue a later one re-files (`--supersedes`, a confirmed repeat)
/// names that row, not just `(closed)`.
#[test]
fn render_names_the_row_that_supersedes_a_closed_one() {
    let mut l = log();
    l.rows.push(row(
        "A0000000000000000000000030",
        "issue",
        &["src/z.rs"],
        None,
    ));
    l.closes
        .push(Row::close("t-0001", "A0000000000000000000000030", "fixed"));
    let mut again = row("A0000000000000000000000031", "issue", &["src/z.rs"], None);
    again.supersedes = Some("A0000000000000000000000030".into());
    l.rows.push(again);
    let all = Filter {
        all: true,
        ..Filter::default()
    };
    let out = render(&l, &find(&l, &all), 10_000);
    assert!(
        out.contains("issue (closed, superseded → A0000000000000000000000031)"),
        "{out}"
    );
}

/// A list says `(closed)`; the full view of a closed row says why — the close
/// text and the commit the closer stamped, or a compacted row's folded text.
#[test]
fn render_full_says_why_a_row_was_closed() {
    let mut l = log();
    l.rows.push(row(
        "A0000000000000000000000030",
        "issue",
        &["src/z.rs"],
        None,
    ));
    let mut close = Row::close(
        "t-0001",
        "A0000000000000000000000030",
        "fixed in  abc1234\nby  the guard",
    );
    close.extra.insert("sha".into(), "abc1234def0".into());
    l.closes.push(close);
    let mut folded = row("A0000000000000000000000031", "issue", &["src/z.rs"], None);
    let why = serde_json::json!({"id": "t-9", "ts": "", "by": "x", "text": "folded reason"});
    folded.extra.insert("closed".into(), why);
    l.rows.push(folded);
    let all = Filter {
        all: true,
        ..Filter::default()
    };
    let rows = find(&l, &all);
    let full = render_full(&l, &rows, 10_000);
    assert!(full.contains("  closed: fixed\n"), "{full}");
    assert!(
        full.contains("  closed: fixed in abc1234 by the guard (abc1234)\n"),
        "{full}"
    );
    assert!(full.contains("  closed: folded reason\n"), "{full}");
    // the list stays one line per row
    assert!(!render(&l, &rows, 10_000).contains("closed:"));
}

#[test]
fn render_shows_urgent_before_to() {
    let mut l = log();
    l.rows.push(Row {
        id: "C0000000000000000000000016".into(),
        kind: "issue".into(),
        text: "hot".into(),
        files: vec!["src/a.rs".into()],
        to: Some("ploy".into()),
        urgent: Some(1.0),
        ..Row::default()
    });
    l.rows.push(Row {
        id: "C0000000000000000000000017".into(),
        kind: "issue".into(),
        text: "half".into(),
        files: vec!["src/a.rs".into()],
        urgent: Some(0.5),
        ..Row::default()
    });
    let out = render(&l, &find(&l, &Filter::default()), 10_000);
    assert!(out.contains("hot (urgent 1, to: ploy) → src/a.rs"), "{out}");
    assert!(out.contains("half (urgent 0.5) → src/a.rs"), "{out}");
}

#[test]
fn est_tokens_counts_thai_per_char() {
    assert_eq!(est_tokens("abcdefgh"), 2);
    assert_eq!(est_tokens("ไทย"), 3);
    // a ULID prefix tokenizes ~2 chars/token, not 4 — ids ride on every row
    assert_eq!(est_tokens("01M3J6HV"), 4);
    assert_eq!(est_tokens("- [01M3J6HV] a"), 2 + 4);
    // plain caps or plain digits are words/numbers, not ids
    assert_eq!(est_tokens("README 20260927"), 4);
}

#[test]
fn abbrev_is_per_row_not_log_wide() {
    let mut l = log();
    l.rows.clear();
    for id in [
        "01M3J6HV00000000000000000A", // same-ms pair: needs 11 chars
        "01M3J6HV00100000000000000B",
        "01M3K0000000000000000000AA", // alone: stays at the 8-char floor
    ] {
        l.rows.push(Row {
            id: id.into(),
            ..Row::default()
        });
    }
    let ab = abbrev(&l);
    assert_eq!(ab.short("01M3J6HV00000000000000000A"), "01M3J6HV000");
    assert_eq!(ab.short("01M3J6HV00100000000000000B"), "01M3J6HV001");
    assert_eq!(ab.short("01M3K0000000000000000000AA"), "01M3K000");
    // a row about to be written counts too
    let ab = abbrev(&l).with("01M3K0000900000000000000ZZ");
    assert_eq!(ab.short("01M3K0000000000000000000AA"), "01M3K00000");
    // every printed prefix resolves back to its row
    for r in &l.rows {
        assert_eq!(resolve(&l, abbrev(&l).short(&r.id)).unwrap().id, r.id);
    }
}

#[test]
fn render_says_how_to_read_a_cut_body() {
    let mut l = log();
    l.rows.push(Row {
        id: "C0000000000000000000000018".into(),
        kind: "note".into(),
        text: "Handoff first. Second sentence the title drops.".into(),
        files: vec!["src/a.rs".into()],
        ..Row::default()
    });
    let hint = "bodies: fael find <id>";
    let out = render(&l, &find(&l, &Filter::default()), 10_000);
    assert!(
        out.contains("Handoff first. …")
            && out.ends_with(&format!(
                "{hint} (MCP: find id=<id>) · every body: --full (MCP: full=true)\n"
            )),
        "{out}"
    );
    // bodies already shown, or nothing cut: no hint
    assert!(!render_full(&l, &find(&l, &Filter::default()), 10_000).contains(hint));
    l.rows.pop();
    assert!(!render(&l, &find(&l, &Filter::default()), 10_000).contains(hint));
}
