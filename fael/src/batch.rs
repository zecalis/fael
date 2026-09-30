//! Filing many rows in one call — `fael add --json -` (a JSON array on
//! stdin) and MCP `rows: [...]`. Split out of main.rs/write.rs (file-size
//! ratchet): parsing here, one shared `add_row` per row over there.

use crate::core::Row;
use crate::hook::{ASK_WARN, record_asks, record_cli_reject, record_row_asks};
use crate::{core, write};
use std::process::ExitCode;

pub(crate) fn written(a: &crate::Args, r: &crate::Repo, row: &Row, path: &std::path::Path) {
    if a.has("json") {
        println!("{}", row.to_line());
    } else {
        let rel = path.strip_prefix(&r.root).unwrap_or(path);
        println!("{} → {}", row.id, rel.display());
    }
}

/// Chunk 6b: batch add — a JSON array on stdin, one object per row
/// (`{kind, text, files[], key?, to?, title?, revisit?, urgent?,
/// urgent_before?, supersedes?, force?}`). Every row runs the same
/// validate + self-heal as a single add; a rejected row reports alone while
/// the rest still save (never all-or-nothing, so no resending the batch).
/// Exit is failure when any row rejected — the saved ones stay saved.
pub(crate) fn batch_add(a: &crate::Args) -> Result<ExitCode, String> {
    if a.has("dry-run") {
        return Err(
            "rejected: --dry-run takes one row — drop --json - and pass <kind> \"<text>\"".into(),
        );
    }
    use std::io::Read;
    let mut stdin = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin)
        .map_err(|e| format!("rejected: cannot read stdin: {e}"))?;
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdin)
        .map_err(|e| format!("rejected: stdin is not a JSON array of rows: {e}"))?;
    if items.is_empty() {
        return Err("rejected: nothing to add — stdin held an empty array".into());
    }
    if !items.iter().all(|v| v.is_object()) {
        return Err("rejected: stdin must be a JSON array of row objects".into());
    }
    let r = crate::repo()?;
    let mut failed = 0;
    for (i, v) in items.iter().enumerate() {
        let parsed = batch_row(v);
        let res = parsed.and_then(|b| {
            write::add_row(&r, &b.kind, &b.text, &b.files, b.opts)
                .map_err(|e| e.trim_start_matches("rejected: ").to_string())
        });
        match res {
            Ok((row, _path, warns)) => {
                warns.iter().for_each(|w| eprintln!("{w}"));
                record_row_asks("cli", "add", &r.root, &row, &warns);
                // batch rides `--json`: one JSON row per line, like single add
                println!("{}", row.to_line());
            }
            Err(e) => {
                failed += 1;
                let e = format!("rejected: row {i}: {e}");
                println!("{e}");
                record_cli_reject("add", &e);
            }
        }
    }
    if failed > 0 {
        Err(format!(
            "rejected: {failed} of {} rows rejected — the rest saved",
            items.len()
        ))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

/// Batch close — `fael close a b "why"` closes each id with the same reason;
/// a bad id reports alone while the rest save (never all-or-nothing, like
/// batch add above). A single id keeps the old behaviour byte for byte: the
/// original error returns unchanged, so `already closed` still lands on stderr.
pub(crate) fn batch_close(a: &crate::Args, ids: &[String], why: &str) -> Result<ExitCode, String> {
    a.only("close", &["json"])?;
    let r = crate::repo()?;
    let (mut failed, mut first) = (0, String::new());
    for id in ids {
        match write::close_row(&r, id, why) {
            Ok((row, path, warns)) => {
                warns.iter().for_each(|w| eprintln!("{w}"));
                record_asks("cli", ASK_WARN, "close", Some(&r.root), &warns);
                written(a, &r, &row, &path);
            }
            Err(e) => {
                failed += 1;
                if first.is_empty() {
                    first = e.clone();
                }
                let e = format!("rejected: {id}: {}", e.trim_start_matches("rejected: "));
                println!("{e}");
                record_cli_reject("close", &e);
            }
        }
    }
    if failed == 0 {
        Ok(ExitCode::SUCCESS)
    } else if ids.len() == 1 {
        Err(first)
    } else {
        Err(format!(
            "rejected: {failed} of {} closes rejected — the rest saved",
            ids.len()
        ))
    }
}

/// Chunk 6b: one batch row (`fael add --json -`, MCP `rows: [...]`) parsed to
/// the same parts as a single add — shared so CLI and MCP never drift.
pub(crate) struct BatchRow {
    pub kind: String,
    pub text: String,
    pub files: Vec<String>,
    pub opts: write::AddOpts,
}

pub(crate) fn batch_row(v: &serde_json::Value) -> Result<BatchRow, String> {
    let one = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(String::from);
    let (kind, text) = match (one("kind"), one("text")) {
        (Some(k), Some(t)) => (k, t),
        _ => return Err("rejected: each row needs kind and text".into()),
    };
    let files = v["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(String::from)
        .collect();
    let urgent = match (v["urgent"].as_bool().unwrap_or(false), one("urgent_before")) {
        (false, None) => core::Urgent::Unset,
        (true, None) => core::Urgent::End,
        (false, Some(t)) => core::Urgent::Before(t),
        (true, Some(_)) => {
            return Err(
                "rejected: urgent and urgent_before pick one — the queue takes a single position"
                    .into(),
            );
        }
    };
    Ok(BatchRow {
        kind,
        text,
        files,
        opts: write::AddOpts {
            key: one("key"),
            to: one("to"),
            title: one("title"),
            revisit: one("revisit"),
            urgent,
            supersedes: one("supersedes"),
            force: v["force"].as_bool().unwrap_or(false),
        },
    })
}
