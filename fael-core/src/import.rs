//! `fael import` — merge another log in, without dropping a row (SPEC §6).
//!
//! The source is a log file, a `.fael/log` dir or a repo root (its
//! `.fael/log`). Output lands in immutable `log/_import/<ULID>.jsonl`, with a
//! `.close.jsonl` companion when closes name no row here: a close folds into
//! its target's `closed` when the target is in the import. A row with no id
//! gets `legacy-<sha1 8 hex of the line>`; unknown fields round-trip through
//! `Row::extra`. Dedupe by id makes importing twice safe.

use crate::compact::fold;
use crate::log::{collect_files, decode_text, dedupe_ids, is_marker, lock, tmp_rename};
use crate::{Row, Store, anchor, ulid};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Opts {
    /// `(old_prefix, new_prefix)` path rewrites for `files` (moving into a
    /// monorepo subfolder); first match wins, anchors never match.
    pub maps: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub adds: usize,
    pub folded: usize,
    pub carried: usize,
    pub skipped: usize,
    pub paths: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Merge `src` into this clone's `_import/`.
///
/// Like every other write, the journal is the commit point: the rows land
/// there first (`journal`, when the clone has one) and in the tree unless
/// `store` is `local`, so a deleted branch cannot take an import with it
/// (01M3HQHJ2).
pub fn import(
    fael: &Path,
    journal: Option<&Path>,
    store: Store,
    src: &Path,
    opts: &Opts,
) -> Result<Report, String> {
    let _guard = lock(fael)?;
    let files = sources(src)?;
    let (mut adds, mut closes) = (vec![], vec![]);
    let mut skipped = 0usize;
    let mut warnings = vec![];
    for f in &files {
        let name = f.to_string_lossy().into_owned();
        let bytes = std::fs::read(f).map_err(|e| format!("{}: {e}", name))?;
        let text = decode_text(&bytes);
        let mut lines: Vec<&str> = text.split('\n').collect();
        lines.pop(); // the torn tail belongs to a live writer, not to us
        let is_close_file = name.ends_with(".close.jsonl");
        for (i, line) in lines.iter().enumerate() {
            let line = line.trim();
            if line.is_empty() || is_marker(line) {
                continue;
            }
            let converted = match crate::secret(line) {
                Some(what) => Err(format!("looks like a secret ({what})")),
                None => convert(line, is_close_file, opts),
            };
            match converted {
                Ok(Converted::Add(r)) => adds.push(r),
                Ok(Converted::Close(r)) => closes.push(r),
                Err(w) => {
                    skipped += 1;
                    if warnings.len() < 5 {
                        warnings.push(format!("{name}:{}: skipped ({w})", i + 1));
                    }
                }
            }
        }
    }
    if adds.is_empty() && closes.is_empty() {
        return Err(format!(
            "{}: no rows to import ({} line(s) skipped{})",
            src.display(),
            skipped,
            warnings
                .first()
                .map(|w| format!(" — first: {w}"))
                .unwrap_or_default()
        ));
    }
    dedupe_ids(&mut adds);
    dedupe_ids(&mut closes);
    let folded = fold(&mut adds, &closes);
    let resolved: std::collections::HashSet<&str> =
        folded.iter().map(|(_, c)| c.id.as_str()).collect();
    let carried: Vec<Row> = closes
        .into_iter()
        .filter(|c| !resolved.contains(c.id.as_str()))
        .collect();

    let stamp = ulid();
    let mut body = String::new();
    for r in &adds {
        body.push_str(&r.to_line());
        body.push('\n');
    }
    let mut cbody = String::new();
    for c in &carried {
        cbody.push_str(&c.to_line());
        cbody.push('\n');
    }
    let close = (!carried.is_empty()).then_some(cbody.as_str());
    let carried_n = carried.len();

    let mut paths = vec![];
    // journal first — the commit point, so a deleted branch cannot take the
    // imported rows with it
    if let Some(j) = journal.filter(|j| *j != fael) {
        let dir = j.join("log").join("_import");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        paths.extend(write_import(&dir, &stamp, &body, close)?);
    }
    // tree next, unless `local` already committed to the journal only
    if !(store == Store::Local && journal.is_some()) {
        let dir = fael.join("log").join("_import");
        let written = std::fs::create_dir_all(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))
            .and_then(|_| write_import(&dir, &stamp, &body, close));
        match written {
            Ok(ps) => paths.extend(ps),
            // the rows are already durable; a retry would import a second copy
            Err(e) if journal.is_some() => warnings.push(format!(
                "warning: tree write failed ({e}) — rows are in the journal; do not retry"
            )),
            Err(e) => return Err(e),
        }
    }
    Ok(Report {
        adds: adds.len(),
        folded: folded.len(),
        carried: carried_n,
        skipped,
        paths,
        warnings,
    })
}

/// Write the import body (and its carried-close companion) into one root's
/// `_import/`, returning the paths written.
fn write_import(
    dir: &Path,
    stamp: &str,
    body: &str,
    close: Option<&str>,
) -> Result<Vec<PathBuf>, String> {
    let out = dir.join(format!("{stamp}.jsonl"));
    tmp_rename(&out, body.as_bytes())?;
    let mut paths = vec![out];
    if let Some(cbody) = close {
        let cout = dir.join(format!("{stamp}.close.jsonl"));
        tmp_rename(&cout, cbody.as_bytes())?;
        paths.push(cout);
    }
    Ok(paths)
}

/// `src` is a file (one log), a `.fael/log` dir or a repo root — whichever
/// holds jsonl wins, in that order.
fn sources(src: &Path) -> Result<Vec<PathBuf>, String> {
    if src.is_file() {
        return Ok(vec![src.to_path_buf()]);
    }
    if !src.is_dir() {
        return Err(format!(
            "rejected: {} is not a file or directory",
            src.display()
        ));
    }
    let nested = src.join(".fael/log");
    let dir = if nested.is_dir() {
        nested
    } else {
        src.to_path_buf()
    };
    let files: Vec<PathBuf> = collect_files(&dir)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    if files.is_empty() {
        return Err(format!("rejected: no .jsonl under {}", src.display()));
    }
    Ok(files)
}

enum Converted {
    Add(Row),
    Close(Row),
}

/// One source line → a fael row, `--map` applied. Anything unparseable or
/// without `by` is an `Err` the caller counts.
fn convert(line: &str, is_close_file: bool, opts: &Opts) -> Result<Converted, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    let Value::Object(mut m) = v else {
        return Err("not a JSON object".into());
    };
    if !m.contains_key("by") {
        return Err("no `by` — not a fael row".into());
    }
    if m.get("id")
        .and_then(Value::as_str)
        .is_none_or(|s| s.is_empty())
    {
        m.insert("id".into(), Value::String(legacy_id(line)));
    }
    let mut row: Row = serde_json::from_value(Value::Object(m)).map_err(|e| e.to_string())?;
    apply_maps(&mut row.files, opts);
    if is_close_file
        || row
            .reference
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty())
    {
        Ok(Converted::Close(row))
    } else {
        Ok(Converted::Add(row))
    }
}

/// `legacy-<first 8 hex of sha1(line)>` — deterministic across machines, so
/// two hosts importing the same legacy log dedupe to the same rows.
fn legacy_id(line: &str) -> String {
    let mut h = Sha1::new();
    h.update(line.trim().as_bytes());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    format!("legacy-{}", &hex[..8])
}

/// Rewrite path prefixes in `files`; anchors are opaque and never match.
fn apply_maps(files: &mut [String], opts: &Opts) {
    if opts.maps.is_empty() {
        return;
    }
    for f in files.iter_mut() {
        if anchor(f).is_some() {
            continue;
        }
        if let Some((old, new)) = opts.maps.iter().find(|(old, _)| f.starts_with(old)) {
            *f = format!("{new}{}", &f[old.len()..]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::legacy_id;

    #[test]
    fn legacy_id_is_sha1() {
        // FIPS 180-1 vector: sha1("abc") = a9993e36…
        assert_eq!(legacy_id("abc"), "legacy-a9993e36");
        assert_eq!(legacy_id("  abc  "), "legacy-a9993e36");
    }
}
