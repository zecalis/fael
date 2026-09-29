//! The pure writer-journal layer for sync (docs/sync-format.md): `Meta`,
//! ref names, month-split tree files, id-union, read-side validation.
//!
//! No git, no network, no clock, no filesystem — the Git transport
//! (`fael/src/sync.rs`, chunk 3) and a future cloud transport both build on
//! this. A transport copies row bytes, never re-renders them.

use crate::{Row, is_month};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// The `format_version` in every `meta.json` this code writes — bump only
/// when a field is removed, renamed, retyped, or changes meaning
/// (docs/sync-format.md); adding an optional field is not a bump.
pub const FORMAT_VERSION: u64 = 1;

/// One ref's provenance label, at the tree root as `meta.json`.
/// Deliberately no `writer` field — the ref already is the writer identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub format_version: u64,
    pub repo_id: String,
    pub origin: String,
    pub name: String,
}

impl Meta {
    /// A v1 label for `repo_id`; `origin`/`name` may be empty when unknown.
    pub fn new(repo_id: &str, origin: &str, name: &str) -> Meta {
        Meta {
            format_version: FORMAT_VERSION,
            repo_id: repo_id.into(),
            origin: origin.into(),
            name: name.into(),
        }
    }

    /// Exact `meta.json` bytes (one object, no trailing newline).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Meta always serialises")
    }

    /// Parse `meta.json` bytes back.
    pub fn from_json(s: &str) -> Result<Meta, String> {
        serde_json::from_str(s).map_err(|e| format!("rejected: meta.json is not JSON ({e})"))
    }
}

/// `refs/fael/<repo-id>/<writer>` — rejects ids that would break a Git ref
/// before any network call. The writer → path-component mapping is identity:
/// no escaping scheme, an unsafe id is an error, never a rewrite.
pub fn ref_name(repo_id: &str, writer: &str) -> Result<String, String> {
    ref_component(repo_id, "repo id")?;
    ref_component(writer, "writer id")?;
    Ok(format!("refs/fael/{repo_id}/{writer}"))
}

/// One flat-tree file: path relative to the ref root plus its exact bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeFile {
    pub path: String,
    pub body: String,
}

/// Group the journal into flat-tree files: `meta.json` first, then one
/// `<yyyy-mm>.jsonl` per month of add rows plus a `<yyyy-mm>.close.jsonl`
/// companion per month of close rows. Month comes from `ts` (UTC), exactly
/// like the local `<writer>/<yyyy-mm>.jsonl` split; row bytes are
/// `Row::to_line`, order kept.
///
/// `rows` and `closes` are the reader's two streams (`.jsonl` vs
/// `.close.jsonl`, [`crate::Log`]) — pass them separately so the tree is the
/// exact inverse of the filename split a reader applies: a close without a
/// `ref` still rides `.close.jsonl`, and an add row with a stray `ref` stays
/// in the month file. Feed every reader-visible row whatever file it came
/// from — `compact.*` and `_import/*` rows travel here re-split by their own
/// `ts` month, and those file names never reach the tree; `quarantine/` and
/// `cache/` are not rows and are never read. Rows whose `ts` has no `yyyy-mm`
/// prefix are skipped: the local append path rejects them, so a journal never
/// holds one.
pub fn tree_files(meta: &Meta, rows: &[Row], closes: &[Row]) -> Vec<TreeFile> {
    let mut out = vec![TreeFile {
        path: "meta.json".into(),
        body: meta.to_json(),
    }];
    let mut months: BTreeMap<String, (Vec<&Row>, Vec<&Row>)> = BTreeMap::new();
    group(rows, false, &mut months);
    group(closes, true, &mut months);
    for (month, (rows, closes)) in &months {
        if !rows.is_empty() {
            out.push(TreeFile {
                path: format!("{month}.jsonl"),
                body: lines(rows),
            });
        }
        if !closes.is_empty() {
            out.push(TreeFile {
                path: format!("{month}.close.jsonl"),
                body: lines(closes),
            });
        }
    }
    out
}

/// Fetched rows missing locally — what ingest appends through the normal
/// write path. Dedupe by `id` is the only mechanism: first occurrence wins,
/// rows already local (or id-less) fall away. Call once per stream (add rows,
/// then close rows) so the two dedupe separately, exactly as the reader does.
pub fn missing(fetched: &[Row], local: &[Row]) -> Vec<Row> {
    let seen: HashSet<&str> = local
        .iter()
        .filter(|r| !r.id.is_empty())
        .map(|r| r.id.as_str())
        .collect();
    let mut out = vec![];
    let mut dup = seen;
    for r in fetched {
        if r.id.is_empty() || !dup.insert(r.id.as_str()) {
            continue;
        }
        out.push(r.clone());
    }
    out
}

/// The merged journal both sides converge on: local rows in order, then the
/// fetched rows missing locally. What a push commits after re-fetching — call
/// once for add rows and once for close rows, then [`tree_files`] the two.
pub fn union(fetched: &[Row], local: &[Row]) -> Vec<Row> {
    let mut out = local.to_vec();
    out.extend(missing(fetched, local));
    out
}

/// Read-side check of a fetched ref's `meta.json`: a supported version and a
/// `repo_id` equal to the ref it came from. A mismatch is rejected on ingest.
pub fn validate(meta: &Meta, repo_id: &str) -> Result<(), String> {
    if meta.format_version != FORMAT_VERSION {
        return Err(format!(
            "rejected: unsupported format_version {} — want {FORMAT_VERSION}",
            meta.format_version
        ));
    }
    if meta.repo_id != repo_id {
        return Err(format!(
            "rejected: meta repo_id {:?} does not match ref {:?}",
            meta.repo_id, repo_id
        ));
    }
    Ok(())
}

fn lines(rows: &[&Row]) -> String {
    let mut body = String::new();
    for r in rows {
        body.push_str(&r.to_line());
        body.push('\n');
    }
    body
}

/// File the rows of one stream into their `ts` month, order kept. `is_close`
/// picks the close companion bucket; rows without a `yyyy-mm` `ts` are skipped.
fn group<'a>(
    src: &'a [Row],
    is_close: bool,
    months: &mut BTreeMap<String, (Vec<&'a Row>, Vec<&'a Row>)>,
) {
    for r in src {
        let Some(month) = r.ts.get(..7).filter(|m| is_month(m)) else {
            continue;
        };
        let slot = months.entry(month.to_string()).or_default();
        if is_close {
            slot.1.push(r);
        } else {
            slot.0.push(r);
        }
    }
}

/// One ref path component: non-empty, no `/` (a writer owns exactly one ref,
/// never a subtree), and nothing `git check-ref-format` forbids.
fn ref_component(s: &str, what: &str) -> Result<(), String> {
    let bad = s.is_empty()
        || s.starts_with(['.', '/'])
        || s.ends_with(['/', '.'])
        || s.ends_with(".lock")
        || s.contains("//")
        || s.contains("..")
        || s.contains("@{")
        || s.contains(['/', '\\'])
        || s.chars()
            .any(|c| c.is_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '['));
    if bad {
        return Err(format!(
            "rejected: {what} {s:?} is not a ref path component — use the writer id as-is, without / .. @{{ space or ~^:?*["
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn row(id: &str, ts: &str, reference: Option<&str>) -> Row {
        Row {
            v: Some(1),
            id: id.into(),
            ts: ts.into(),
            by: "alice-3f9a".into(),
            kind: if reference.is_some() {
                String::new()
            } else {
                "note".into()
            },
            text: format!("row {id}"),
            files: if reference.is_some() {
                vec![]
            } else {
                vec!["a.rs".into()]
            },
            reference: reference.map(str::to_string),
            ..Row::default()
        }
    }

    fn meta() -> Meta {
        Meta::new("abc123", "https://example.test/r", "r")
    }

    #[test]
    fn round_trip_rows_through_tree_files() {
        let rows = vec![
            row(
                "01J8ZQ3K400000000000000001",
                "2026-09-15T10:00:00.000Z",
                None,
            ),
            row(
                "01J8ZQ3K400000000000000002",
                "2026-09-16T10:00:00.000Z",
                None,
            ),
            row(
                "01J8ZQ3K400000000000000004",
                "2026-10-01T10:00:00.000Z",
                None,
            ),
        ];
        let closes = vec![row(
            "01J8ZQ3K400000000000000003",
            "2026-09-17T10:00:00.000Z",
            Some("01J8ZQ3K400000000000000001"),
        )];
        let files = tree_files(&meta(), &rows, &closes);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "meta.json",
                "2026-09.jsonl",
                "2026-09.close.jsonl",
                "2026-10.jsonl"
            ]
        );
        assert_eq!(Meta::from_json(&files[0].body), Ok(meta()));
        let mut back = vec![];
        let mut warns = vec![];
        for f in &files[1..] {
            parse(f.body.as_bytes(), &f.path, &mut back, &mut warns);
        }
        assert!(warns.is_empty(), "{warns:?}");
        let mut want: Vec<String> = rows.iter().chain(&closes).map(|r| r.to_line()).collect();
        let mut got: Vec<String> = back.iter().map(|r| r.to_line()).collect();
        want.sort();
        got.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn stream_split_follows_the_reader_not_the_ref_field() {
        // a ref-less close (an import can carry one) still rides .close.jsonl
        let mut no_ref = row(
            "01J8ZQ3K400000000000000021",
            "2026-09-15T10:00:00.000Z",
            None,
        );
        no_ref.kind.clear();
        no_ref.files.clear();
        // an add row with a stray `ref` stays in the month file
        let mut stray = row(
            "01J8ZQ3K400000000000000022",
            "2026-09-15T10:00:00.000Z",
            None,
        );
        stray.reference = Some("01J8ZQ3K400000000000000021".into());
        let files = tree_files(&meta(), &[stray], &[no_ref]);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["meta.json", "2026-09.jsonl", "2026-09.close.jsonl"]);
        assert!(files[1].body.contains("01J8ZQ3K400000000000000022"));
        assert!(files[2].body.contains("01J8ZQ3K400000000000000021"));
    }

    #[test]
    fn carriers_ride_the_main_stream() {
        let mut moved = row(
            "01J8ZQ3K400000000000000011",
            "2026-09-15T10:00:00.000Z",
            None,
        );
        moved.kind.clear();
        moved.files.clear();
        moved.extra.insert(
            "moved".into(),
            serde_json::json!({"from": "a.rs", "to": "b.rs"}),
        );
        let mut restored = row(
            "01J8ZQ3K400000000000000012",
            "2026-09-15T10:00:00.000Z",
            None,
        );
        restored.kind.clear();
        restored.files.clear();
        restored.restores = Some("01J8ZQ3K400000000000000011".into());
        let files = tree_files(&meta(), &[moved, restored], &[]);
        assert_eq!(files.len(), 2); // meta.json + one month file, no .close file
        assert_eq!(files[1].path, "2026-09.jsonl");
    }

    #[test]
    fn ref_name_ok_and_rejected() {
        assert_eq!(
            ref_name("abc123", "alice-3f9a"),
            Ok("refs/fael/abc123/alice-3f9a".into())
        );
        for bad in [
            "", "a/b", ".a", "a..b", "..", "a b", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "a\\b",
            "a@{b", "a.lock", "a/", "a.",
        ] {
            assert!(ref_name("abc123", bad).is_err(), "{bad:?}");
            assert!(ref_name(bad, "alice-3f9a").is_err(), "repo {bad:?}");
        }
    }

    #[test]
    fn union_dedupes_by_id_first_wins() {
        let a = row(
            "01J8ZQ3K400000000000000001",
            "2026-09-15T10:00:00.000Z",
            None,
        );
        let mut a2 = a.clone();
        a2.text = "forked copy".into();
        let b = row(
            "01J8ZQ3K400000000000000002",
            "2026-09-16T10:00:00.000Z",
            None,
        );
        let local = vec![a.clone()];
        let fetched = vec![a2, b.clone(), b.clone()];
        let miss = missing(&fetched, &local);
        assert_eq!(miss, vec![b.clone()]);
        assert_eq!(union(&fetched, &local), vec![a, b]);
    }

    #[test]
    fn validate_meta_rejects_version_and_repo_mismatch() {
        assert!(validate(&meta(), "abc123").is_ok());
        let mut v2 = meta();
        v2.format_version = 2;
        assert!(validate(&v2, "abc123").is_err());
        assert!(validate(&meta(), "other").is_err());
    }
}
