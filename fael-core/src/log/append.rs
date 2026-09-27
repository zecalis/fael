//! The write side of `.fael/log/**`: locking, add/bump/close/mv and the raw
//! append. Moved out of `log.rs` (file-size ratchet) — no logic of its own.

use super::{Log, is_month};
use crate::{
    Config, Row, Stamp, Store, Urgent, UrgentChange, closed, resolve, resolve_urgent, superseded,
    validate, validate_alias, validate_close, warnings,
};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A month file this big refuses appends — run `fael compact` (well under GitHub's 50 MiB warning is the point).
pub const MONTH_MAX: u64 = 50 * 1024 * 1024;

/// The open file ends mid-line (no trailing `\n`) — write one before the next
/// append, or the new row glues onto a torn line. An empty file is an `Err`
/// (the backward seek fails): `append` checks the length first and propagates
/// real IO errors; the hook's stop-block file is fail-open, so any `Err` there
/// just means "no seal".
/// Shared by `append` and the hook's stop-block file.
pub fn needs_seal(f: &mut fs::File) -> std::io::Result<bool> {
    let mut last = [0u8];
    f.seek(SeekFrom::End(-1))
        .and_then(|_| f.read_exact(&mut last))
        .map(|_| last[0] != b'\n')
}

/// Keep `.fael/.lock` out of git (format.md §Layout): whoever takes the lock
/// makes sure `.fael/.gitignore` names it — appended to an owner's file, never
/// rewriting it. Fail-open: a write error only leaves the file as it was.
// ponytail: one small read per append; the hook read/edit path never appends
fn ignore_lock(fael: &Path) {
    let path = fael.join(".gitignore");
    let cur = fs::read_to_string(&path).unwrap_or_default();
    if cur.lines().any(|l| l.trim() == ".lock") {
        return;
    }
    let sep = if cur.is_empty() || cur.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(format!("{sep}.lock\n").as_bytes()));
}

/// Hold `.fael/.lock` across a multi-step rewrite (`compact`, `doctor --fix`)
/// — the same lock single appends take.
pub(crate) fn lock(fael: &Path) -> Result<std::fs::File, String> {
    std::fs::create_dir_all(fael).map_err(|e| format!("{}: {e}", fael.display()))?;
    ignore_lock(fael);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(fael.join(".lock"))
        .map_err(|e| format!("lock: {e}"))?;
    lock.lock().map_err(|e| format!("lock: {e}"))?;
    Ok(lock)
}

/// Write-then-rename in the same directory (atomic on POSIX/NTFS).
pub(crate) fn tmp_rename(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use crate::ulid;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".fael-tmp-{}", ulid()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

/// Validate an add row, then append it to `<fael>/log/<by>/<yyyy-mm>.jsonl`.
pub fn add(fael: &Path, row: &Row, cfg: &Config) -> Result<PathBuf, String> {
    validate(row, cfg)?;
    append(fael, row, false)
}

/// Validate a close row, then append it to `<fael>/log/<by>/<yyyy-mm>.close.jsonl`.
pub fn close(fael: &Path, row: &Row, cfg: &Config) -> Result<PathBuf, String> {
    validate_close(row, cfg)?;
    append(fael, row, true)
}

/// Resolve `supersedes`, stamp, validate, append — the one add path every adapter (CLI, MCP,
/// a server) goes through. `row.files` must already be normalised. Returns the non-fatal warnings.
///
/// `journal` is the clone-shared journal root (`<git-common-dir>/fael`, resolved
/// without a spawn on the adapter side; `None` without git). The journal is the
/// commit point: it is written first and its failure fails the whole write.
/// The tree follows per `store` — `local` skips it, `tracked` degrades its
/// failure to a one-line warning (the row is already durable; a retry would
/// file a second row with a new id).
pub fn add_row(
    fael: &Path,
    journal: Option<&Path>,
    log: &Log,
    cfg: &Config,
    stamp: &Stamp,
    mut row: Row,
    supersedes: Option<&str>,
) -> Result<(Row, PathBuf, Vec<String>), String> {
    if let Some(s) = supersedes {
        row.supersedes = Some(resolve(log, s)?.id.clone());
    }
    stamp.apply(&mut row);
    let (path, mut warns) = write_both(fael, journal, cfg, |d| add(d, &row, cfg))?;
    warns.extend(warnings(&row, log, cfg));
    Ok((row, path, warns))
}

/// Journal-first write shared by add/close/mv: `put` writes one validated row
/// to one root (tree or journal — same line bytes both places). Returns the
/// path the row landed in (the journal one when the tree is skipped or fails)
/// plus a warning when a `tracked` tree write fails after the journal commit.
fn write_both(
    fael: &Path,
    journal: Option<&Path>,
    cfg: &Config,
    put: impl Fn(&Path) -> Result<PathBuf, String>,
) -> Result<(PathBuf, Vec<String>), String> {
    let jpath = match journal {
        Some(j) => Some(put(j)?),
        None => None,
    };
    // `local` with a journal skips the tree; without one (no git) the tree
    // is all there is and the write falls through to it
    if matches!(cfg.store, Store::Local)
        && let Some(p) = jpath
    {
        return Ok((p, vec![]));
    }
    match put(fael) {
        Ok(p) => Ok((p, vec![])),
        Err(e) => match jpath {
            Some(p) => Ok((
                p,
                vec![format!(
                    "warning: tree write failed ({e}) — row is in the journal; do not retry"
                )],
            )),
            None => Err(e),
        },
    }
}

/// What `bump_row` changes — bundled so the arg count stays under the lint
/// (the binary's `AddOpts` does the same for `add_row`). `to`/`revisit`:
/// `None` keeps the old value, `Some("")` clears it, anything else sets it.
pub struct BumpOpts {
    pub to: Option<String>,
    pub urgent: UrgentChange,
    pub revisit: Option<String>,
}

/// Change routing/urgency/revisit on an open row as a new version (MVCC-style):
/// the same kind, title, text, files and key, new `to`/`urgent`/`revisit`,
/// superseding the old row — the one add path every adapter (CLI, MCP, a
/// server) goes through, so the old version hides through `superseded()` with
/// no new visibility rule. Text and files never change through bump — file a
/// new row for new content.
pub fn bump_row(
    fael: &Path,
    journal: Option<&Path>,
    log: &Log,
    cfg: &Config,
    stamp: &Stamp,
    id: &str,
    opts: BumpOpts,
) -> Result<(Row, PathBuf, Vec<String>), String> {
    let BumpOpts {
        to,
        urgent,
        revisit,
    } = opts;
    let old = resolve(log, id)?.clone();
    if closed(log).contains(old.id.as_str()) {
        return Err(format!(
            "rejected: {} is already closed — bump an open row",
            old.id
        ));
    }
    if superseded(log).contains(old.id.as_str()) {
        return Err(format!(
            "rejected: {} is already superseded — bump the newer version",
            old.id
        ));
    }
    let to = match to {
        None => old.to.clone(),
        Some(t) => {
            let t = t.trim().to_lowercase();
            (!t.is_empty()).then_some(t)
        }
    };
    let urgent = match urgent {
        UrgentChange::Keep => old.urgent,
        UrgentChange::End => resolve_urgent(log, &Urgent::End)?,
        UrgentChange::Before(t) => resolve_urgent(log, &Urgent::Before(t))?,
        UrgentChange::Remove => None,
    };
    let revisit = match revisit {
        None => old.revisit.clone(),
        Some(v) => {
            let v = v.trim().to_string();
            (!v.is_empty()).then_some(v)
        }
    };
    let mut row = Row::new(&stamp.by, &old.kind, &old.text, old.files.clone());
    row.key = old.key.clone();
    row.title = old.title.clone();
    row.to = to;
    row.urgent = urgent;
    row.revisit = revisit;
    add_row(fael, journal, log, cfg, stamp, row, Some(&old.id))
}

/// Resolve `id`, stamp, validate, append a close row. A row already closed is rejected.
pub fn close_row(
    fael: &Path,
    journal: Option<&Path>,
    log: &Log,
    cfg: &Config,
    stamp: &Stamp,
    id: &str,
    why: &str,
) -> Result<(Row, PathBuf, Vec<String>), String> {
    let target = resolve(log, id)?;
    // a second close row adds nothing but noise to an append-only log
    if closed(log).contains(target.id.as_str()) {
        return Err(format!("rejected: {} is already closed", target.id));
    }
    // bump makes id churn routine: closing the hidden old version would leave
    // the live one open, so point at the newest version instead
    if superseded(log).contains(target.id.as_str()) {
        let mut newest = target.id.as_str();
        while let Some(n) = log
            .rows
            .iter()
            .find(|r| r.supersedes.as_deref() == Some(newest))
        {
            newest = &n.id;
        }
        return Err(format!(
            "rejected: {} is superseded — close the newest version {newest}",
            target.id
        ));
    }
    let mut row = Row::close(&stamp.by, &target.id, why);
    stamp.apply(&mut row);
    let (path, warns) = write_both(fael, journal, cfg, |d| close(d, &row, cfg))?;
    Ok((row, path, warns))
}

/// Build, stamp, validate and append an alias row (`fael mv <old> <new>`).
/// `from`/`to` must already be normalised. Returns the row, its file and the
/// non-fatal warnings (a `tracked` tree write that failed after the journal
/// commit — same contract as `add_row`/`close_row`).
pub fn mv_row(
    fael: &Path,
    journal: Option<&Path>,
    cfg: &Config,
    stamp: &Stamp,
    from: &str,
    to: &str,
) -> Result<(Row, PathBuf, Vec<String>), String> {
    let mut row = Row::moved(&stamp.by, from, to);
    stamp.apply(&mut row);
    validate_alias(&row, cfg)?;
    let (path, warns) = write_both(fael, journal, cfg, |d| append(d, &row, false))?;
    Ok((row, path, warns))
}

/// Append without validating (import/compact write already-checked rows through here).
/// lock → seal a torn tail with `\n` → one `write_all` of the whole line → unlock on drop.
pub fn append(fael: &Path, row: &Row, is_close: bool) -> Result<PathBuf, String> {
    // `by` and the month become path parts — refuse anything that could leave the writer folder
    let by = &row.by;
    if by.is_empty() || by.starts_with(['_', '.']) || by.contains(['/', '\\']) {
        return Err(format!("rejected: writer id {by:?} is not a folder name"));
    }
    let month = row.ts.get(..7).filter(|m| is_month(m));
    let Some(month) = month else {
        return Err(format!("rejected: ts {:?} is not RFC 3339", row.ts));
    };
    let dir = fael.join("log").join(by);
    let path = dir.join(format!(
        "{month}{}.jsonl",
        if is_close { ".close" } else { "" }
    ));
    let io = |e: std::io::Error| format!("{}: {e}", path.display());

    fs::create_dir_all(&dir).map_err(io)?;
    ignore_lock(fael);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(fael.join(".lock"))
        .map_err(io)?;
    lock.lock().map_err(io)?;

    let mut f = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&path)
        .map_err(io)?;
    let len = f.metadata().map_err(io)?.len();
    if len >= MONTH_MAX {
        return Err(format!(
            "rejected: {} is ≥ 50 MiB — run `fael compact` first",
            path.display()
        ));
    }
    let line = row.to_line();
    let mut buf = Vec::with_capacity(line.len() + 2);
    if len > 0 && needs_seal(&mut f).map_err(io)? {
        buf.push(b'\n'); // seal the torn line off; readers then skip it as a broken line
    }
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    f.write_all(&buf).map_err(io)?;
    Ok(path)
}
