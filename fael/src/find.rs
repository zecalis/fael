//! Read-side CLI: `find` (with id lookup + `--full`), `kickoff`, `keys`.
//! Moved out of main.rs (file-size ratchet) — no logic of its own beyond the
//! title/body split: lists show titles, `find <id>` and `--full` show bodies.

pub(crate) mod branches;
pub(crate) mod issue;
mod kickoff;
pub(crate) mod many;
pub(crate) mod mentions;
mod merged;
pub(crate) mod misses;

pub(crate) use kickoff::kickoff;

use super::{Args, aliases};
use fael_core::{self as core, Filter, Log, Row};

pub(crate) fn find(a: &Args, text: Option<&String>) -> Result<(), String> {
    let r = super::repo()?;
    // the union read tags journal-only rows with their stamped branch;
    // `--branches` merges unmerged branches' rows on top of it (HEAD wins on
    // duplicate ids) and tags those ` @<branch>` on render
    let (base, jtags) = super::journal::read(&r);
    // an id-shaped query is an id lookup, never text — unless `--text`
    // forces a text search (the escape hatch for the old fallback)
    let forced = a.one("text");
    if forced.is_none()
        && let Some(t) = text
        && core::looks_like_id(t)
    {
        let (log, wide, btags) = super::refs::resolve_wide(&r, base, t);
        return match wide {
            super::refs::Wide::One(row) => {
                // the body was just printed — the next push must not say it again
                super::hook::note_seen(&super::session::hook_session(&r.root), &r.root, &[&row.id]);
                found(&r.root, &row.id);
                show_one(a, &log, &row, &super::journal::overlay(jtags, btags))
            }
            super::refs::Wide::Many(rows) => Err(reject_many(t, &rows)),
            super::refs::Wide::Missing => Err(reject_missing(&log, t)),
        };
    }
    let (log, branch_of) = working_or_branches(a, base, jtags, &r);
    // `fael find <id>` pulls the body: an exact id or unique prefix wins over
    // text search (a text query equalling a unique id prefix means the id)
    if forced.is_none()
        && let Some(t) = text
        && let Ok(row) = core::resolve_row(&log, t)
    {
        found(&r.root, &row.id);
        return show_one(a, &log, row, &branch_of);
    }
    // `find plan:x` reads the anchor the way `kickoff plan:x` does
    let (query, anchor, text_key) = text_or_anchor(&log, forced.as_ref(), text, &r);
    let mut files = core::normalize_files(&a.files(), &r.cwd, &r.root)?;
    files.extend(anchor);
    let (limit, offset) = a.paging()?;
    let f = Filter {
        text: query.cloned(),
        files: aliases::load(&r, &log, true).expand_all(&files),
        key: a.one("key").or(text_key),
        kind: a.one("kind"),
        since: a.one("since"),
        by: a.one("by"),
        to: a.one("to").map(|t| t.trim().to_lowercase()),
        // bare `--revisit` = any revisit, `--revisit=<text>` narrows to it
        revisit: a
            .has("revisit")
            .then(|| a.one("revisit").unwrap_or_default()),
        all: a.has("all"),
        limit,
        offset,
    };
    if a.has("groups") || issue::auto_grouped(a, &f) {
        return issue::groups(a, &log, &f, branch_of, &r);
    }
    let (rows, budget, total) = core::query(&log, &f, &r.cfg);
    if rows.is_empty() {
        eprintln!(
            "fael: {}",
            misses::explain(&r.root, "cli", &log, &f, "--files")
        );
        return Ok(());
    }
    // a list of one or two shows its bodies: the next call would be `find <id>`
    let full = a.has("full") || core::expands(&rows, total, &f, budget);
    // the cut line reprints this call with the next offset — same flags, no
    // guessing; under --full (bodies fill the budget in a few rows) it asks
    // for the rest in one call, since an explicit --limit beats the budget
    let base = a.page_base("find", query.or(text).map(String::as_str), limit);
    let rest_in_one = a.has("full") && limit.is_none();
    let next = |n: usize| match rest_in_one {
        true => format!("{base} --offset {n} --limit {}", total.saturating_sub(n)),
        false => format!("{base} --offset {n}"),
    };
    let shown = show(
        a,
        full,
        &log,
        &rows,
        budget,
        core::Cut {
            total,
            offset,
            next: &next,
        },
        &branch_of,
    )?;
    super::hook::record_found(
        "cli",
        "find",
        &r.root,
        &shown,
        (f.key.as_deref(), &files, None),
    );
    // chunk 6e: ids just shown for these files are already in this session's
    // context — the next push skips them instead of repeating them
    if !files.is_empty() {
        let ids: Vec<&str> = shown.iter().map(String::as_str).collect();
        super::hook::note_seen(&super::session::hook_session(&r.root), &r.root, &ids);
    }
    // --all in JSON: also the close rows naming a shown row, so a consumer can tell closed from open
    if a.has("json") && f.all {
        let open: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        log.closes
            .iter()
            .filter(|c| c.reference.as_deref().is_some_and(|id| open.contains(id)))
            .for_each(|c| println!("{}", c.to_line()));
    }
    Ok(())
}

/// A body pulled by id: the outcome a `bodies:` line earns on.
fn found(root: &std::path::Path, id: &str) {
    super::hook::record_found(
        "cli",
        "find",
        root,
        &[id.to_string()],
        (None, &[], Some(id)),
    );
}

/// A text query that is exactly a file or anchor some row is filed on
/// (`plan:x`, a path) is that file, not words — the reading `kickoff` gives
/// it. One that is exactly a key some row carries — bare, as the `#k` a row
/// prints, or `key:k` — is that key: text never searches keys. Anything
/// else, or `forced` (`--text`), stays a text search. Returns `(text query,
/// files to add, key)`.
pub(crate) fn text_or_anchor<'a>(
    log: &Log,
    forced: Option<&'a String>,
    text: Option<&'a String>,
    r: &super::Repo,
) -> (Option<&'a String>, Vec<String>, Option<String>) {
    if forced.is_some() {
        return (forced, vec![], None);
    }
    let Some(t) = text else {
        return (None, vec![], None);
    };
    let k = t
        .strip_prefix('#')
        .or_else(|| t.strip_prefix("key:"))
        .unwrap_or(t);
    // a bare word that is also a key (`usage`) stays words; `scope:name` is a key
    if (k != t || k.contains(':')) && log.rows.iter().any(|row| row.key.as_deref() == Some(k)) {
        return (None, vec![], Some(k.to_string()));
    }
    match core::normalize_files(std::slice::from_ref(t), &r.cwd, &r.root).ok() {
        Some(f) if log.rows.iter().any(|row| row.files.contains(&f[0])) => (None, f, None),
        _ => (text, vec![], None),
    }
}

/// The union log, or the union log plus unmerged branches' rows when
/// `--branches` is passed (HEAD wins on duplicate ids — a merged-then-listed
/// row never doubles, never tags). Read/edit push never pass the flag: no git
/// spawn belongs on the 5 ms push path.
pub(super) fn working_or_branches(
    a: &Args,
    log: Log,
    journal: branches::BranchMap,
    r: &super::Repo,
) -> (Log, branches::BranchMap) {
    if a.has("branches") {
        let (log, tags, note) = branches::widen(r, log, journal);
        if let Some(n) = note {
            eprintln!("{n}");
        }
        (log, tags)
    } else {
        (log, journal)
    }
}

pub(super) fn show(
    a: &Args,
    full: bool,
    log: &Log,
    rows: &[&Row],
    budget: usize,
    cut: core::Cut,
    branch_of: &branches::BranchMap,
) -> Result<Vec<String>, String> {
    if rows.is_empty() {
        eprintln!("fael: no rows match");
        return Ok(vec![]);
    }
    if a.has("json") {
        // JSON stays the row shape (consumers dedupe by id) — the branch tag
        // is a list-display feature, like titles
        rows.iter().for_each(|r| println!("{}", r.to_line()));
        if let Some(n) = core::json_note(rows, budget) {
            eprintln!("fael: {n}");
        }
        return Ok(rows.iter().map(|r| r.id.clone()).collect());
    }
    // only what fit the budget was said — like the push, the cut rows may come
    // on a later read, so seen-ids take just the shown lines
    let out = if full {
        branches::tag(core::render_full_page(log, rows, budget, cut), branch_of)
    } else {
        branches::tag(core::render_page(log, rows, budget, cut), branch_of)
    };
    let n = out.lines().filter(|l| l.starts_with("- [")).count();
    print!("{out}");
    Ok(rows.iter().take(n).map(|r| r.id.clone()).collect())
}

/// One row pulled by id: always the body (`render_full`), or the JSON line —
/// plus the rows that merely mention the id, so the reader sees who cites it
/// without knowing a flag for it (chunk 4). Silent when nobody cites it.
fn show_one(a: &Args, log: &Log, row: &Row, branch_of: &branches::BranchMap) -> Result<(), String> {
    if a.has("json") {
        println!("{}", row.to_line());
    } else {
        // `--branches` tags a row that only lives on another branch, same as a list
        print!(
            "{}",
            branches::tag(core::render_full(log, &[row], 10_000), branch_of)
        );
        let who = mentions::mentioners(log, row);
        if !who.is_empty() {
            println!("mentioned by: {}", who.join(", "));
        }
    }
    Ok(())
}

/// An id-shaped query matching ≥2 rows: an abbreviation that decayed as the
/// log grew — it exists, only ambiguous. Same wording as `core::resolve`.
pub(crate) fn reject_many(tok: &str, rows: &[Row]) -> String {
    format!(
        "rejected: id {tok:?} matches {} rows ({}) — use more characters",
        rows.len(),
        rows.iter()
            .take(5)
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// An id-shaped query with no row behind it (union and unmerged branches):
/// the reject, plus up to 5 rows that only mention the string — a text hit
/// is not the row existing.
pub(crate) fn reject_missing(log: &Log, tok: &str) -> String {
    let mut m = format!("rejected: no row with id {tok:?} — copy the id from fael find");
    let who = mentions::mentioned(log, tok);
    if !who.is_empty() {
        m.push_str(&format!("\nmentioned (not owned) by: {}", who.join(", ")));
    }
    m
}

pub(crate) fn keys(a: &Args, pattern: Option<&String>) -> Result<(), String> {
    let r = super::repo()?;
    let log = super::read(&r);
    let ks = core::keys(&log, pattern.map(String::as_str));
    if ks.is_empty() {
        eprintln!("fael: no keys yet");
    }
    for k in ks {
        if a.has("json") {
            println!(
                "{}",
                serde_json::json!({"key": k.key, "count": k.count, "last": k.last})
            );
        } else {
            println!(
                "- {} ×{} (last {})",
                k.key,
                k.count,
                k.last.get(..10).unwrap_or(&k.last)
            );
        }
    }
    Ok(())
}

// Moved out of main.rs (file-size ratchet) — no logic of its own.
impl Args {
    /// Rebuild this `find`/`kickoff` call for the cut line: the same filters,
    /// so the agent reruns it with the new `--offset` the renderer appends.
    /// Kickoff takes no filter flags, only `--full` and `--limit`.
    pub(crate) fn page_base(
        &self,
        cmd: &str,
        positional: Option<&str>,
        limit: Option<usize>,
    ) -> String {
        let mut s = format!("fael {cmd}");
        if let Some(p) = positional.filter(|p| !p.is_empty()) {
            s.push_str(&format!(" {}", quoted(p)));
        }
        if cmd == "find" {
            let fs = self.files();
            if !fs.is_empty() {
                s.push_str(&format!(" --files {}", quoted(&fs.join(","))));
            }
            // `--text` replays as the flag — the positional alone would id-match
            if let Some(t) = self.one("text") {
                s.push_str(&format!(" --text {}", quoted(&t)));
            }
            for f in ["key", "kind", "since", "by", "to"] {
                if let Some(v) = self.one(f) {
                    s.push_str(&format!(" --{f} {}", quoted(&v)));
                }
            }
            // bare `--revisit` replays bare; a value replays with it
            if self.has("revisit") {
                match self.one("revisit") {
                    Some(v) => s.push_str(&format!(" --revisit {}", quoted(&v))),
                    None => s.push_str(" --revisit"),
                }
            }
            if self.has("all") {
                s.push_str(" --all");
            }
        }
        // `--branches` replays on both find and kickoff — the cut line keeps
        // the branch rows across pages instead of silently dropping them
        if self.has("branches") {
            s.push_str(" --branches");
        }
        if self.has("full") {
            s.push_str(" --full");
        }
        if let Some(n) = limit {
            s.push_str(&format!(" --limit {n}"));
        }
        s
    }
}

/// Quote only when the shell would need it — `--kind issue` stays bare, a
/// glob (`--key auth:*`) or `$x` is single-quoted so the shell passes it as-is.
pub(crate) fn quoted(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || "-_./:,@+=".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}
