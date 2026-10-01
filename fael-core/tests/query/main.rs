//! find · brief · keys · render · resolve · warnings · urgent · bump · title · revisit — against an in-memory `Log`.
//!
//! Thin entry only — the suites sit next to this file:
//! `select` (find/brief/push/gone/kickoff/`to`), `render` (render/tokens),//! `paging` (limit/offset page + cut line), `lookup` (resolve/keys/warnings/glob), `keyhint` (prompt → open key, exact segment only),
//! `urgent` (queue/6-step rank), `bump` (MVCC-style new versions),
//! `title` (title/body split, `--title` fallback,
//! `render_full`), `revisit` (`--revisit` dates vs text, due kickoff, `find --revisit`),
//! `focus` (push buckets + row cap), `precision` (push golden fixture: tiers + injected ids).
//! `carrier` (no-kind-no-files rows never list, legacy stays).
//! `search` (chunk 4 scenarios: files + text, key glob, blank text).
//! `restore` (superseded = edges minus reverted; restore rows are carriers;
//! old readers over-hide by the raw edge).
//! The shared builders live here.

mod anchor;
mod bump;
mod carrier;
mod focus;
mod keyhint;
mod lookup;
mod md;
mod paging;
mod precision;
mod refs;
mod render;
mod restore;
mod revisit;
mod search;
mod select;
mod stale;
mod title;
mod urgent;

use fael_core::*;

fn row(id: &str, kind: &str, files: &[&str], key: Option<&str>) -> Row {
    Row {
        id: id.into(),
        ts: format!("2026-09-{}T00:00:00Z", &id[id.len() - 2..]),
        kind: kind.into(),
        text: format!("text of {id}"),
        files: files.iter().map(|s| s.to_string()).collect(),
        key: key.map(String::from),
        ..Row::default()
    }
}

fn log() -> Log {
    let mut sup = row(
        "A0000000000000000000000014",
        "decision",
        &["src/a.rs"],
        Some("auth:session"),
    );
    sup.supersedes = Some("A0000000000000000000000011".into());
    Log {
        rows: vec![
            row(
                "A0000000000000000000000010",
                "issue",
                &["src/a.rs"],
                Some("auth:session"),
            ),
            row(
                "A0000000000000000000000011",
                "decision",
                &["src/a.rs"],
                None,
            ),
            row(
                "A0000000000000000000000012",
                "note",
                &["src/sub/b.rs"],
                Some("billing:invoice"),
            ),
            row(
                "A0000000000000000000000013",
                "issue",
                &[".\\src\\c.rs"],
                None,
            ), // legacy spelling
            sup,
            row(
                "B0000000000000000000000015",
                "note",
                &["doc:pricing/2026"],
                None,
            ),
        ],
        closes: vec![Row::close("t-0000", "A0000000000000000000000010", "fixed")],
        warnings: vec![],
    }
}

fn ids(rows: &[&Row]) -> Vec<String> {
    rows.iter().map(|r| r.id[24..].to_string()).collect()
}

fn files(f: &[&str]) -> Filter {
    Filter {
        files: f.iter().map(|s| s.to_string()).collect(),
        ..Filter::default()
    }
}
