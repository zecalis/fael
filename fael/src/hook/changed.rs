//! Which open rows' files changed since the row was written
//! (PLAN-fael-file-hash chunk 2). The row's `fh` map holds each real file's
//! git blob id at write time; this module compares those against the bytes on
//! disk now, resolved through L1 aliases (a rename reads the new path).
//! Pure disk reads, no git spawn — safe on the 5 ms push path. A row with no
//! `fh`, or a file that resolves nowhere, is unknown, never changed.

use crate::core;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Files bigger than this are never hashed on the push path (the stamp side
/// covers up to 16 MiB — 01M42CGE). A file over this cap has no verdict here,
/// even when stamped: hashing it would break the 5 ms push ceiling. `doctor`
/// reads the same number to list the rows this leaves without a verdict.
pub(crate) const PUSH_MAX_BYTES: u64 = 1024 * 1024;

/// Files this big are still compared on pulls (`kickoff`, `doctor`): the stamp
/// side covers this much (01M42CGE), and a pull has no 5 ms ceiling to break.
/// One source with the stamp cap — `filehash::MAX_BYTES` under a pull-side name.
pub(crate) const STAMP_MAX_BYTES: u64 = crate::filehash::MAX_BYTES;

/// One blob cache per push, so each file is read once however many rows name
/// it (hub files carry dozens of rows) and the edit hint and the shadow split
/// share the reads.
pub(crate) type Blobs = HashMap<String, Option<String>>;

/// The shadow split: full ids of the `changed` rows and of the `unchanged` rows.
pub(crate) type Split = (Vec<String>, Vec<String>);

/// A row against the worktree. `Changed` carries the first stamped file (as it
/// lives now) that differs; `Unknown` is a row with no `fh` or a file that
/// resolves nowhere (gone, renamed in a cycle, unreadable or over the push cap
/// — `[Gone]` owns the gone case, never "changed").
enum Verdict {
    Changed(String),
    Same,
    Unknown,
}

/// The shadow split for a push's usage line (PLAN-fael-file-hash chunk 3):
/// full ids of the shown rows whose files changed since the row was written,
/// and full ids of the shown rows whose files all still match. Rows with no
/// verdict ride in neither list — never guessed as changed. Nothing renders:
/// the caller records the two lists on the usage line only.
fn partition(rows: &[&core::Row], root: &Path, al: &core::Aliases, blobs: &mut Blobs) -> Split {
    let mut changed_ids = vec![];
    let mut unchanged_ids = vec![];
    for r in rows {
        match verdict(r, root, al, blobs, &[], PUSH_MAX_BYTES) {
            Verdict::Changed(_) => changed_ids.push(r.id.clone()),
            Verdict::Same => unchanged_ids.push(r.id.clone()),
            Verdict::Unknown => {}
        }
    }
    (changed_ids, unchanged_ids)
}

/// Ids of the handoff rows (`key` ending `:handoff`) whose code files moved
/// since the row was written (PLAN-fael-file-hash chunk 5a): the kickoff
/// label. The same row verdict as the edit hint, but with the stamp-side cap —
/// kickoff is a pull with no 5 ms ceiling — and skipping the note's own plan
/// file: `plan_anchor` reads it as the plan, and it moves every chunk, so
/// counting it would label every handoff. One blob cache for the call; rows
/// with no verdict stay out — unknown, never changed.
pub(crate) fn handoff_changed(
    rows: &[&core::Row],
    root: &Path,
    al: &core::Aliases,
    prefixes: &[String],
) -> HashSet<String> {
    let mut blobs: Blobs = HashMap::new();
    rows.iter()
        .filter(|r| r.key.as_deref().is_some_and(|k| k.ends_with(":handoff")))
        .filter(|r| {
            r.file_hashes()
                .filter(|fh| !fh.is_empty())
                .is_some_and(|fh| {
                    fh.iter().any(|(f, v)| {
                        if core::plan_anchor(f, prefixes).is_some() {
                            return false;
                        }
                        let (Some(want), Some(target)) = (v.as_str(), resolve(al, root, f)) else {
                            return false;
                        };
                        file_verdict(want, &target, root, &mut blobs, STAMP_MAX_BYTES)
                            .is_some_and(|d| d)
                    })
                })
        })
        .map(|r| r.id.clone())
        .collect()
}

/// `only` narrows the row to the stamped files that live at one of these
/// paths now (an edit's files); empty reads every stamped file. A row with no
/// stamp among them is unknown. `cap` bounds one hashed file: the push path
/// passes `PUSH_MAX_BYTES`, pulls the stamp-side cap.
fn verdict(
    row: &core::Row,
    root: &Path,
    al: &core::Aliases,
    blobs: &mut Blobs,
    only: &[String],
    cap: u64,
) -> Verdict {
    let Some(fh) = row.file_hashes().filter(|fh| !fh.is_empty()) else {
        return Verdict::Unknown;
    };
    let (mut unknown, mut read) = (false, 0);
    for (f, v) in fh {
        let (Some(want), Some(target)) = (v.as_str(), resolve(al, root, f)) else {
            unknown |= only.is_empty();
            continue;
        };
        if !only.is_empty() && !only.contains(&target) {
            continue;
        }
        read += 1;
        match file_verdict(want, &target, root, blobs, cap) {
            Some(true) => return Verdict::Changed(target),
            Some(false) => {}
            None => unknown = true,
        }
    }
    if unknown || read == 0 {
        Verdict::Unknown
    } else {
        Verdict::Same
    }
}

/// Where `f` lives now: through L1 renames, else itself — a path that exists
/// again after being renamed away (a split) is read as itself. `None` when the
/// renames cycle, so there is no single answer — unknown, never changed.
fn resolve(al: &core::Aliases, root: &Path, f: &str) -> Option<String> {
    match al.current(f) {
        Some(t) if !root.join(f).is_file() => Some(t),
        Some(_) => Some(f.to_string()),
        None if al.forward(f).len() > 1 => None,
        None => Some(f.to_string()),
    }
}

/// One stamped file against disk: `None` when it is gone, a directory,
/// unreadable, or over `cap` — never guessed as changed.
fn file_verdict(
    want: &str,
    target: &str,
    root: &Path,
    blobs: &mut Blobs,
    cap: u64,
) -> Option<bool> {
    let blob = blobs
        .entry(target.to_string())
        .or_insert_with(|| blob_at(root, target, cap));
    blob.as_ref().map(|now| now != want)
}

/// The 12-hex blob id on disk, or `None` when there is nothing hashable.
/// Files over `PUSH_MAX_BYTES` have no verdict: the stamp side covers up to 16
/// MiB, but hashing that much on the push path would break its 5 ms ceiling
/// (01M42CGE) — unknown keeps the legacy hint, never a false "changed".
fn blob_at(root: &Path, target: &str, cap: u64) -> Option<String> {
    let md = std::fs::metadata(root.join(target)).ok()?;
    if !md.is_file() {
        return None;
    }
    if md.len() > cap {
        return None;
    }
    let mut file = std::fs::File::open(root.join(target)).ok()?;
    core::blob_id_stream(&mut file, cap).ok().flatten()
}

/// The said rows' ids plus, on a read, their shadow split (PLAN-fael-file-hash
/// chunk 3): what fit the budget, of which `changed` files moved since the row
/// was written and `unchanged` still match — rows with no verdict ride
/// neither. An edit gets no split: the file on disk already holds the edit.
pub(crate) fn split_said(
    sel: &core::Selection<'_>,
    n: usize,
    edit: bool,
    root: &Path,
    al: &core::Aliases,
    blobs: &mut Blobs,
) -> (Vec<String>, Option<Split>) {
    let said: Vec<&core::Row> = sel.shown.iter().take(n).copied().collect();
    let shown: Vec<String> = said.iter().map(|r| r.id.clone()).collect();
    (shown, (!edit).then(|| partition(&said, root, al, blobs)))
}

/// What an edit push knows about the session: the files it edited (`files`,
/// the only ones a hint may call changed), rows already in the agent's
/// context (`told`), rows an earlier edit hint already named (`hinted`, the
/// `~<id>` lines of the seen list), and who is asking (`session`, so its own
/// rows are left out).
pub(crate) struct Ask<'a> {
    pub log: &'a core::Log,
    pub root: &'a Path,
    pub files: &'a [String],
    pub al: &'a core::Aliases,
    pub session: &'a str,
    pub told: &'a HashSet<String>,
    pub hinted: &'a HashSet<String>,
}

/// The hint text and the keys it spent: the caller appends `~<key>` for each,
/// so the same row is never named twice in a session.
pub(crate) struct Hint {
    pub text: String,
    pub spent: Vec<String>,
}

/// The seen list split into the ids said and the keys already hinted.
/// `@<id>` (in-context at edit) and `^<id>` (cited) lines are neither.
pub(crate) fn read_seen(old: &str) -> (HashSet<String>, HashSet<String>) {
    let (mut told, mut hinted) = (HashSet::new(), HashSet::new());
    for l in old.lines() {
        if let Some(k) = l.strip_prefix('~') {
            hinted.insert(k.to_string());
        } else if !l.starts_with(['@', '^']) {
            told.insert(l.to_string());
        }
    }
    (told, hinted)
}

/// The tier-0 rows of an edit push the agent has in front of it — said now
/// (`said`) or by an earlier push this session (`ask.told`) — through
/// `stale_hint`. A row cut by the cap or the budget was never said, so it is
/// never named, and neither is a row this very session filed: the agent just
/// wrote it, so asking whether it is still true after its own edit says
/// nothing new.
pub(crate) fn edit_hint(
    ask: &Ask,
    t0: &[(&core::Row, usize)],
    said: &[&core::Row],
    blobs: &mut Blobs,
) -> Option<Hint> {
    let rows: Vec<&core::Row> = t0
        .iter()
        .filter(|(r, tier)| {
            *tier == 0
                && (ask.told.contains(&r.id) || said.iter().any(|s| s.id == r.id))
                && (ask.session.is_empty() || !own_row(r, ask.session))
        })
        .map(|(r, _)| *r)
        .collect();
    stale_hint(ask, &rows, blobs)
}

/// Did the hook's session file `r`? Claude keys the hook by its transcript
/// path while the row carries the bare session id — the path's stem — so
/// either counts (as stats attribute it, 01M3V9QAN).
fn own_row(r: &core::Row, session: &str) -> bool {
    r.session()
        .is_some_and(|s| s == session || Path::new(session).file_stem().is_some_and(|f| *f == *s))
}

/// The edit hint over the tier-0 rows already in the agent's context: rows
/// whose edited file changed since the row was written are named (at most
/// two, each with the changed file and the retire ready to run); rows whose
/// edited file matches earn no hint; rows with no verdict are named too (two
/// at most, issues first: an open issue with a ready close, any other row
/// with the retire) — beside the changed rows only open issues. Every ask names its
/// row: an ask with no id is one the agent cannot act on. A row said by an
/// earlier edit hint this session is not said again. `None` when no hint is
/// earned.
///
/// The edit hook runs after the write (PostToolUse), so "changed since the row
/// was written" includes the edit just made: a row filed before it is the one
/// to re-check — once, since the next edit would only repeat the same ask.
fn stale_hint(ask: &Ask, rows: &[&core::Row], blobs: &mut Blobs) -> Option<Hint> {
    let mut changed_rows: Vec<(&core::Row, String)> = vec![];
    let mut unknown_rows: Vec<&core::Row> = vec![];
    for r in rows.iter().filter(|r| !ask.hinted.contains(&r.id)) {
        match verdict(r, ask.root, ask.al, blobs, ask.files, PUSH_MAX_BYTES) {
            Verdict::Changed(f) => changed_rows.push((r, f)),
            Verdict::Same => {}
            Verdict::Unknown => unknown_rows.push(r),
        }
    }
    changed_rows.truncate(2);
    let named = !changed_rows.is_empty();
    // issues first, the only ones said beside changed rows; two at most
    unknown_rows.sort_by_key(|r| r.kind != "issue");
    unknown_rows.retain(|r| !named || r.kind == "issue");
    unknown_rows.truncate(2);
    let (issues, others): (Vec<&core::Row>, Vec<&core::Row>) =
        unknown_rows.into_iter().partition(|r| r.kind == "issue");
    let mut lines = vec![];
    if named {
        lines.push(changed_hint(ask.log, &changed_rows));
    }
    if !issues.is_empty() {
        lines.push(close_hint(ask.log, &issues));
    }
    if !others.is_empty() {
        lines.push(retire_hint(ask.log, &others));
    }
    let spent: Vec<String> = changed_rows
        .iter()
        .map(|(r, _)| *r)
        .chain(issues.iter().copied())
        .chain(others.iter().copied())
        .map(|r| r.id.clone())
        .collect();
    (!lines.is_empty()).then(|| Hint {
        text: lines.join("\n"),
        spent,
    })
}

/// `src/a.rs changed since <id> was written`, each with the bump, the
/// supersede re-file and the close ready to run. Two rows share one line and
/// one command tail (`<id>` stands for either) — the same words said once.
fn changed_hint(log: &core::Log, rows: &[(&core::Row, String)]) -> String {
    let ab = core::abbrev(log);
    let said: Vec<String> = rows
        .iter()
        .map(|(r, file)| format!("{file} changed since {} was written", ab.short(&r.id)))
        .collect();
    let s = match rows {
        [(r, _)] => ab.short(&r.id).to_string(),
        _ => "<id>".to_string(),
    };
    format!(
        "fael: {} — still true? `fael bump {s}` · wrong now? re-file with `--supersedes {s}` · done? `fael close {s} \"now in <file>\"`",
        said.join(" · ")
    )
}

/// The ready close for open issues with no verdict (at most two).
fn close_hint(log: &core::Log, issues: &[&core::Row]) -> String {
    let ab = core::abbrev(log);
    let calls: Vec<String> = issues
        .iter()
        .map(|r| format!("fael close {} \"<why>\"", ab.short(&r.id)))
        .collect();
    format!("fael: done with one? {}", calls.join(" · "))
}

/// The retire for other rows with no verdict (at most two), named like
/// `changed_hint` names its rows: `<id>` in the commands stands for either.
fn retire_hint(log: &core::Log, rows: &[&core::Row]) -> String {
    let ab = core::abbrev(log);
    let ids: Vec<&str> = rows.iter().map(|r| ab.short(&r.id)).collect();
    let s = match ids[..] {
        [one] => one,
        _ => "<id>",
    };
    format!(
        "fael: does the code now say or contradict {}? `fael close {s} \"now in <file>\"` · or re-file with `--supersedes {s}`",
        ids.join(" · ")
    )
}
