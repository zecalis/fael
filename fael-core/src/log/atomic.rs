//! Atomic multi-row writes (c1): one `.lock` across rows + the add+close
//! helper that validates both gates before writing anything.

use super::{is_month, lock};
use crate::{Config, Row, Stamp, needs_seal};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The open file ends mid-line — shared with `append` (same seal rule).
fn needs_seal_here(f: &mut std::fs::File) -> std::io::Result<bool> {
    let mut last = [0u8];
    f.seek(SeekFrom::End(-1))
        .and_then(|_| f.read_exact(&mut last))
        .map(|_| last[0] != b'\n')
}

/// `by`/month for one row — validated for every row before any write, so a
/// bad row aborts the batch with nothing written.
fn parts(row: &Row, is_close: bool) -> Result<(String, String), String> {
    let by = &row.by;
    if by.is_empty() || by.starts_with(['_', '.']) || by.contains(['/', '\\']) {
        return Err(format!("rejected: writer id {by:?} is not a folder name"));
    }
    let Some(month) = row.ts.get(..7).filter(|m| is_month(m)) else {
        return Err(format!("rejected: ts {:?} is not RFC 3339", row.ts));
    };
    Ok((
        by.clone(),
        format!("{month}{}.jsonl", if is_close { ".close" } else { "" }),
    ))
}

/// Append many rows under one `.lock` (c1): validate all paths first, then
/// hold the lock across every `write_all`. Callers validate row content
/// (`validate`/`validate_close`, both gates for `add_close`) before calling,
/// so a gate fault writes nothing.
pub fn append_many(fael: &Path, rows: &[(&Row, bool)]) -> Result<Vec<PathBuf>, String> {
    let planned: Vec<(String, String, String)> = rows
        .iter()
        .map(|(r, c)| parts(r, *c).map(|(by, name)| (by, name, r.to_line())))
        .collect::<Result<_, _>>()?;
    let _guard = lock(fael)?;
    let mut out = vec![];
    for ((by, name, line), _) in planned.iter().zip(rows.iter()) {
        let dir = fael.join("log").join(by);
        let path = dir.join(name);
        let io = |e: std::io::Error| format!("{}: {e}", path.display());
        std::fs::create_dir_all(&dir).map_err(io)?;
        let mut f = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .map_err(io)?;
        let len = f.metadata().map_err(io)?.len();
        if len >= super::append::MONTH_MAX {
            return Err(format!(
                "rejected: {} is ≥ 50 MiB — run `fael compact` first",
                path.display()
            ));
        }
        let mut buf = Vec::with_capacity(line.len() + 2);
        if len > 0 && needs_seal_here(&mut f).map_err(io)? {
            buf.push(b'\n');
        }
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        f.write_all(&buf).map_err(io)?;
        out.push(path);
    }
    let _ = needs_seal;
    Ok(out)
}

/// File one content row and close one row atomically (c1): both gates
/// (`add_gate` for the new row, `close_gate` for the close reason) are
/// checked before any write, then both rows land under one lock.
/// `supersedes` retires the old row like `add_row`; `close` is
/// `(id, why)` for the separate close in the same atomic unit.
pub type AddCloseOut = ((Row, PathBuf), (Row, PathBuf), Vec<String>);

pub fn add_close(
    fael: &Path,
    log: &crate::Log,
    cfg: &Config,
    stamp: &Stamp,
    mut row: Row,
    supersedes: Option<&str>,
    close: (&str, &str),
) -> Result<AddCloseOut, String> {
    let sup_id = match supersedes {
        Some(s) => Some(super::append::resolve_content_id(log, s)?.to_string()),
        None => None,
    };
    let target = super::append::resolve_content_id(log, close.0)?.to_string();
    let target_kind = log
        .rows
        .iter()
        .find(|r| r.id == target)
        .map(|r| r.kind.clone())
        .unwrap_or_default();
    crate::add_gate(&row, log, cfg, sup_id.as_deref())?;
    crate::close_gate(&target, close.1, &target_kind)?;
    row.supersedes = sup_id;
    stamp.apply(&mut row);
    let mut close_row = Row::close(&stamp.by, &target, close.1);
    stamp.apply(&mut close_row);
    crate::validate(&row, cfg)?;
    crate::validate_close(&close_row, cfg)?;
    let refs: Vec<(&Row, bool)> = vec![(&row, false), (&close_row, true)];
    let mut paths = append_many(fael, &refs)?;
    let cp = paths.pop().expect("two rows");
    let ap = paths.pop().expect("two rows");
    crate::work::elsewhere::register_store(fael, None);
    Ok(((row, ap), (close_row, cp), vec![]))
}
