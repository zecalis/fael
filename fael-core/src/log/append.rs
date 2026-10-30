//! The write side of `.fael/log/**`: locking, add/bump/close/mv and the raw
//! append. Moved out of `log.rs` (file-size ratchet) — no logic of its own.

use super::{Log, is_month};
use crate::{
    Config, Ref, Row, Stamp, closed, resolve_identity, reverted, superseded, validate,
    validate_alias, validate_close, warnings,
};
use std::collections::HashSet;
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

/// One content row by exact id or unique prefix, through the canonical
/// identity resolver (c2, F-IDENT) — never a private fallback. Carriers and
/// close rows are not supersede/close targets: naming one rejects like a miss
/// so the caller retries with the content id `fael find` printed.
fn resolve_content<'a>(log: &'a Log, input: &str) -> Result<&'a Row, String> {
    match resolve_identity(log, input) {
        Ref::One(r) if crate::is_carrier_row(r) => Err(format!(
            "rejected: no row with id {input:?} — copy the id from fael find (this session's output, never from memory)"
        )),
        Ref::One(r) if log.closes.iter().any(|c| c.id == r.id) => Err(format!(
            "rejected: no row with id {input:?} — copy the id from fael find (this session's output, never from memory)"
        )),
        Ref::One(r) => Ok(r),
        Ref::Many(hits) => Err(format!(
            "rejected: id {input:?} matches {} rows ({}) — use more characters",
            hits.len(),
            hits.iter()
                .take(5)
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Ref::Missing => Err(format!(
            "rejected: no row with id {input:?} — copy the id from fael find (this session's output, never from memory)"
        )),
    }
}

/// Whitespace-normalised, lowercased text — the duplicate comparison core
/// shares with the binary's recall (same content retries name the survivor).
fn norm(t: &str) -> String {
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// One content-row id through the canonical resolver — what supersede and
/// close targets resolve with (c1 uses c2's resolver, never a fallback).
pub(crate) fn resolve_content_id<'a>(log: &'a Log, input: &'a str) -> Result<&'a str, String> {
    Ok(resolve_content(log, input)?.id.as_str())
}
fn existing_path(dir: &Path, row: &Row, is_close: bool) -> PathBuf {
    let month = row.ts.get(..7).filter(|m| is_month(m)).unwrap_or("unknown");
    dir.join("log").join(&row.by).join(format!(
        "{month}{}.jsonl",
        if is_close { ".close" } else { "" }
    ))
}

/// Marker every idempotent no-op warn carries — the binary skips hook tally
/// and usage recording when it is present (retry emits zero new events).
pub const NOOP_MARK: &str = "— not filed again";

/// Resolve `supersedes`, stamp, validate, append — the one add path every adapter (CLI, MCP,
/// a server) goes through. `row.files` must already be normalised. Returns the non-fatal warnings.
///
/// `journal` is the clone-shared journal root (`<git-common-dir>/fael`, resolved
/// without a spawn on the adapter side; `None` without git). The journal is the
/// only store: the tree log is written only without one (no git).
///
/// Idempotent (c1, T5): superseding an already-superseded row writes nothing
/// and hands back the live survivor with a `NOOP_MARK` warn, so a retry after
/// a timeout emits zero new events. Superseding a closed-but-open-headed row
/// still files (an explicit reopen, counted as a confirmed repeat).
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
        let target = resolve_content(log, s)?.id.clone();
        // T5: already superseded -> a retry (same content as the survivor)
        // names the winner with zero new rows. Different content files a new
        // edge (multi-edge, disambiguated by `restore --edge`): dropping an
        // explicit caller's content would be inference.
        if superseded(log).contains(target.as_str()) {
            let surv_id = newest_version(log, target.as_str()).to_string();
            if let Some(surv) = log.rows.iter().find(|r| r.id == surv_id) {
                let same = surv.kind == row.kind
                    && surv.key == row.key
                    && norm(&surv.text) == norm(&row.text)
                    && surv.files.iter().collect::<std::collections::BTreeSet<_>>()
                        == row.files.iter().collect::<std::collections::BTreeSet<_>>();
                if same {
                    let surv = surv.clone();
                    let path = existing_path(journal.unwrap_or(fael), &surv, false);
                    return Ok((
                        surv,
                        path,
                        vec![format!(
                            "already superseded: {target} by {surv_id} {NOOP_MARK}"
                        )],
                    ));
                }
            }
        }
        row.supersedes = Some(target);
    }
    stamp.apply(&mut row);
    let path = write_both(fael, journal, |d| add(d, &row, cfg))?;
    let warns = warnings(&row, log, cfg);
    Ok((row, path, warns))
}

/// The one write shared by add/close/mv/restore: `put` writes one validated row
/// to the journal, or to the tree when there is no journal (no git). A tree
/// log already in the repo is frozen history — read, never written.
/// A successful write registers the repo (fail-open), so cross-repo lookup
/// needs no scan: the ledger exists locally only if fael wrote it.
pub(crate) fn write_both(
    fael: &Path,
    journal: Option<&Path>,
    put: impl Fn(&Path) -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
    let path = put(journal.unwrap_or(fael))?;
    crate::work::elsewhere::register_store(fael, journal);
    Ok(path)
}

/// Resolve `id`, stamp, validate, append a close row.
///
/// Idempotent (c1, T2/T3): a retry writes zero events and hands back the
/// canonical result — the existing close row (T2, already closed) or the live
/// survivor (T3, superseded with an open head) — with a `NOOP_MARK` warn.
/// A superseded row whose head is already closed is the stuck-chain repair:
/// it still writes (one close row per open version, oldest first, atomic).
///
/// Closing the newest version of a bumped chain also closes every version it
/// supersedes — one close row each, oldest first, the requested row last. A
/// supersede marker hides a row for good (format.md §Readers), so without the
/// chain-close the older versions would stay hidden with no close row while the
/// newest one is closed: the whole chain unreachable through `fael close`.
///
/// The reverse guard is relaxed to match: a superseded `id` is closable when
/// its newest version is already closed (the old "close the newest version"
/// reject no longer applies — it *is* closed). Still a normal close row, so
/// every older reader hides it exactly as before.
pub fn close_row(
    fael: &Path,
    journal: Option<&Path>,
    log: &Log,
    cfg: &Config,
    stamp: &Stamp,
    id: &str,
    why: &str,
) -> Result<(Row, PathBuf, Vec<String>), String> {
    let target = resolve_content(log, id)?.id.clone();
    // T2: a second close writes nothing but names the existing close row
    if closed(log).contains(target.as_str()) {
        let existing = log
            .closes
            .iter()
            .find(|c| c.reference.as_deref() == Some(target.as_str()))
            .cloned()
            .ok_or_else(|| format!("rejected: {target} is already closed"))?;
        let path = existing_path(journal.unwrap_or(fael), &existing, true);
        return Ok((
            existing,
            path,
            vec![format!("already closed: {target} {NOOP_MARK}")],
        ));
    }
    // T3: superseded with an open head — point at the survivor, write nothing
    if superseded(log).contains(target.as_str()) {
        let newest = newest_version(log, target.as_str()).to_string();
        if !closed(log).contains(newest.as_str()) {
            let surv = log
                .rows
                .iter()
                .find(|r| r.id == newest)
                .cloned()
                .ok_or_else(|| format!("rejected: no row with id {id:?}"))?;
            let path = existing_path(journal.unwrap_or(fael), &surv, false);
            return Ok((
                surv,
                path,
                vec![format!(
                    "superseded by {newest} {NOOP_MARK} — close {newest} instead"
                )],
            ));
        }
    }
    let mut pending: Vec<Row> = vec![];
    // oldest first, the requested row last (its id is the chain's last entry)
    for r in chain_versions(log, target.as_str()) {
        if closed(log).contains(r.as_str()) {
            continue;
        }
        let mut row = Row::close(&stamp.by, &r, why);
        stamp.apply(&mut row);
        // the `shown` stamp names the requested row's target, not this one: an
        // older version closed by the chain was never itself shown (r1)
        if r != target {
            row.extra.remove("shown");
        }
        validate_close(&row, cfg)?;
        pending.push(row);
    }
    let Some(last) = pending.last().cloned() else {
        return Err(format!("rejected: {target} is already closed"));
    };
    // one .lock across the chain: an interrupt never leaves a half-closed
    // chain behind, so a retry is always a clean T2 no-op above
    let dir = journal.unwrap_or(fael);
    let refs: Vec<(&Row, bool)> = pending.iter().map(|r| (r, true)).collect();
    let mut paths = super::atomic::append_many(dir, &refs)?;
    crate::work::elsewhere::register_store(fael, journal);
    Ok((
        last,
        paths.pop().expect("at least one close row lands"),
        vec![],
    ))
}

/// The versions a close of `id` must cover, oldest first and `id` last — the
/// `supersedes` chain through `id` (cycle-tolerant on a hand-edited log). A
/// reverted edge is skipped, so the row a restore reopened is never swept into
/// a close of the superseder above it.
fn chain_versions(log: &Log, id: &str) -> Vec<String> {
    let rev = reverted(log);
    let mut cur = id.to_string();
    let mut seen = HashSet::from([cur.clone()]);
    let mut out = vec![cur.clone()];
    while let Some(prev) = log
        .rows
        .iter()
        .find(|r| r.id == cur)
        .and_then(|r| r.supersedes.clone())
    {
        if rev.contains(cur.as_str()) || !seen.insert(prev.clone()) {
            break;
        }
        cur = prev;
        out.push(cur.clone());
    }
    out.reverse();
    out
}

/// The live end of a chain: `id` when nothing supersedes it, else the newest
/// version reachable through supersede rows (cycle-tolerant like `chain_versions`).
fn newest_version<'a>(log: &'a Log, id: &'a str) -> &'a str {
    let mut newest = id;
    let mut seen = HashSet::from([newest]);
    while let Some(n) = log
        .rows
        .iter()
        .find(|r| r.supersedes.as_deref() == Some(newest))
    {
        if !seen.insert(n.id.as_str()) {
            break;
        }
        newest = n.id.as_str();
    }
    newest
}

/// Build, stamp, validate and append an alias row (`fael mv <old> <new>`).
/// `from`/`to` must already be normalised. Returns the row, its file and no
/// warnings (same shape as `add_row`/`close_row`).
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
    let path = write_both(fael, journal, |d| append(d, &row, false))?;
    Ok((row, path, vec![]))
}

/// Append without validating (import/compact write already-checked rows through here).
/// One `.lock` across the write via `atomic::append_many`.
pub fn append(fael: &Path, row: &Row, is_close: bool) -> Result<PathBuf, String> {
    let mut paths = super::atomic::append_many(fael, &[(row, is_close)])?;
    Ok(paths.pop().expect("one row"))
}
