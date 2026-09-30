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
    /// `origin` loses any `user:token@` — the label travels to every reader.
    pub fn new(repo_id: &str, origin: &str, name: &str) -> Meta {
        Meta {
            format_version: FORMAT_VERSION,
            repo_id: repo_id.into(),
            origin: strip_userinfo(origin),
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

/// `scheme://user:pass@host/p` → `scheme://host/p`. scp-style `git@host:p`
/// has no password slot and passes through, like every url without `://`.
fn strip_userinfo(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.into();
    };
    let host = &rest[..rest.find('/').unwrap_or(rest.len())];
    match host.rfind('@') {
        Some(at) => format!("{scheme}://{}", &rest[at + 1..]),
        None => url.into(),
    }
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
mod tests;
