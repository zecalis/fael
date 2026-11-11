//! A brief cite that resolves to no row or to several (chunk dead-cite):
//! a warning only, never a refusal. A live row stays silent.

use std::collections::HashSet;

/// Warnings for id-shaped tokens in `title`/`brief` with no row behind them
/// (`Missing`) or several (`Many`). Live rows stay silent; closed rows are
/// said elsewhere (`stale_warnings`). Chunk uids share the ULID shape, so a
/// `Missing` token that names a chunk is no ledger cite.
pub(crate) fn warnings(
    title: &str,
    brief: &str,
    log: &fael_core::Log,
    store: &fael_crew::Store,
) -> Vec<String> {
    let text = format!("{title} {brief}");
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for tok in fael_core::id_tokens(&text) {
        if !seen.insert(tok.to_ascii_uppercase()) {
            continue;
        }
        match fael_core::ref_state(log, tok) {
            fael_core::Ref::One(_) => {}
            fael_core::Ref::Many(rows) => out.push(many_line(tok, &rows)),
            fael_core::Ref::Missing if store.is_chunk(tok) => {}
            fael_core::Ref::Missing => {
                out.push(format!("warning: brief cites {tok} — no such row"));
            }
        }
    }
    out
}

/// One ambiguous-cite warning: the token plus every row it matches, so the
/// planner sees where it settled. Ids sorted for a stable line.
fn many_line(tok: &str, rows: &[&fael_core::Row]) -> String {
    let mut ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    ids.sort_unstable();
    format!(
        "warning: brief cites {tok} — matches {} rows: {}",
        ids.len(),
        ids.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::warnings;

    fn row(id: &str) -> fael_core::Row {
        fael_core::Row {
            id: id.into(),
            kind: "issue".into(),
            text: "x".into(),
            ..Default::default()
        }
    }

    fn log_two() -> fael_core::Log {
        fael_core::Log {
            rows: vec![
                row("01M0000000000000000000001"),
                row("01M0000000000000000000002"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_missing_token_warns_and_a_live_row_stays_silent() {
        let dir = std::env::temp_dir().join(format!("fael-dead-{}", fael_core::ulid()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("crew.db");
        let store = fael_crew::Store::open(&db).unwrap();
        let log = log_two();
        let w = warnings(
            "t",
            "see 01M0000000000000000000001 and 01M0000000000000000000099",
            &log,
            &store,
        );
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("cites 01M0000000000000000000099") && w[0].contains("no such row"),
            "{}",
            w[0]
        );
    }

    #[test]
    fn an_ambiguous_prefix_warns_with_both_ids() {
        let dir = std::env::temp_dir().join(format!("fael-dead-{}", fael_core::ulid()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("crew.db");
        let store = fael_crew::Store::open(&db).unwrap();
        let log = log_two();
        let w = warnings("t", "see 01M0000000000000", &log, &store);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("01M0000000000000000000001")
                && w[0].contains("01M0000000000000000000002"),
            "{}",
            w[0]
        );
    }

    #[test]
    fn a_chunk_uid_is_no_ledger_cite() {
        let dir = std::env::temp_dir().join(format!("fael-dead-{}", fael_core::ulid()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("crew.db");
        let mut store = fael_crew::Store::open(&db).unwrap();
        let uid = store
            .add(
                "inbox",
                &fael_crew::Fields {
                    title: Some("T".to_string()),
                    ..Default::default()
                },
                &[],
                &fael_core::rfc3339(fael_core::now_ms()),
            )
            .unwrap();
        let log = log_two();
        let w = warnings("t", &format!("see {uid}"), &log, &store);
        assert!(w.is_empty(), "{w:?}");
    }
}
