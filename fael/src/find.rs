//! Read-side CLI: `find` (with id lookup + `--full`), `kickoff`, `keys`.
//! Moved out of main.rs (file-size ratchet) — no logic of its own beyond the
//! title/body split: lists show titles, `find <id>` and `--full` show bodies.

pub(crate) mod branches;

use super::{Args, aliases};
use fael_core::{self as core, Filter, Log, Row};

pub(crate) fn find(a: &Args, text: Option<&String>) -> Result<(), String> {
    let r = super::repo()?;
    // the union read tags journal-only rows with their stamped branch;
    // `--branches` merges unmerged branches' rows on top of it (HEAD wins on
    // duplicate ids) and tags those ` @<branch>` on render
    let (base, jtags) = super::journal::read(&r);
    let (log, branch_of) = working_or_branches(a, base, jtags, &r.root);
    // `fael find <id>` pulls the body: an exact id or unique prefix wins over
    // text search (a text query equalling a unique id prefix means the id)
    if let Some(t) = text
        && let Ok(row) = core::resolve(&log, t)
    {
        return show_one(a, &log, row, &branch_of);
    }
    let files = core::normalize_files(&a.files(), &r.cwd, &r.root)?;
    let (limit, offset) = a.paging()?;
    let f = Filter {
        text: text.cloned(),
        files: aliases::load(&r, &log, true).expand_all(&files),
        key: a.one("key"),
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
    let (rows, budget, total) = core::query(&log, &f, &r.cfg);
    // the cut line reprints this call with the next offset — same flags, no guessing
    let base = a.page_base("find", text.map(String::as_str), limit);
    show(
        a,
        &log,
        &rows,
        budget,
        core::Cut {
            total,
            offset,
            next: &|n| format!("{base} --offset {n}"),
        },
        &branch_of,
    )?;
    // --all in JSON: also the close rows naming a shown row, so a consumer can tell closed from open
    if a.has("json") && f.all {
        let shown: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        log.closes
            .iter()
            .filter(|c| c.reference.as_deref().is_some_and(|id| shown.contains(id)))
            .for_each(|c| println!("{}", c.to_line()));
    }
    Ok(())
}

pub(crate) fn kickoff(a: &Args, anchor: Option<&String>) -> Result<(), String> {
    let r = super::repo()?;
    let files = core::normalize_files(&Vec::from_iter(anchor.cloned()), &r.cwd, &r.root)?;
    let (base, jtags) = super::journal::read(&r);
    let (log, branch_of) = working_or_branches(a, base, jtags, &r.root);
    let al = aliases::load(&r, &log, true);
    let (limit, offset) = a.paging()?;
    let f = Filter {
        files: al.expand_all(&files),
        limit,
        offset,
        ..Filter::default()
    };
    // kickoff ranks the full set itself, so it pages after — same helper as query()
    let (rows, total) = core::page(core::kickoff(&log, &f, &r.root, &al), limit, offset);
    let base = a.page_base("kickoff", anchor.map(String::as_str), limit);
    show(
        a,
        &log,
        &rows,
        r.cfg.kickoff_tokens,
        core::Cut {
            total,
            offset,
            next: &|n| format!("{base} --offset {n}"),
        },
        &branch_of,
    )?;
    // free-text revisits never list — one count line points at them
    // (due dates list in full above, so they need no line)
    if !a.has("json") && offset == 0 {
        let n = core::waiting(&log).len();
        if n > 0 {
            print!("{}", core::waiting_line(n));
        }
    }
    Ok(())
}

/// The union log, or the union log plus unmerged branches' rows when
/// `--branches` is passed (HEAD wins on duplicate ids — a merged-then-listed
/// row never doubles, never tags). Read/edit push never pass the flag: no git
/// spawn belongs on the 5 ms push path.
fn working_or_branches(
    a: &Args,
    log: Log,
    journal: branches::BranchMap,
    root: &std::path::Path,
) -> (Log, branches::BranchMap) {
    if a.has("branches") {
        let (log, btags) = branches::with_branches(root, log);
        (log, super::journal::overlay(journal, btags))
    } else {
        (log, journal)
    }
}

fn show(
    a: &Args,
    log: &Log,
    rows: &[&Row],
    budget: usize,
    cut: core::Cut,
    branch_of: &branches::BranchMap,
) -> Result<(), String> {
    if rows.is_empty() {
        eprintln!("fael: no rows match");
    } else if a.has("json") {
        // JSON stays the row shape (consumers dedupe by id) — the branch tag
        // is a list-display feature, like titles
        rows.iter().for_each(|r| println!("{}", r.to_line()));
    } else if a.has("full") {
        print!(
            "{}",
            branches::tag(
                core::render_full_page(log, rows, budget, cut),
                log,
                branch_of
            )
        );
    } else {
        print!(
            "{}",
            branches::tag(core::render_page(log, rows, budget, cut), log, branch_of)
        );
    }
    Ok(())
}

/// One row pulled by id: always the body (`render_full`), or the JSON line.
fn show_one(a: &Args, log: &Log, row: &Row, branch_of: &branches::BranchMap) -> Result<(), String> {
    if a.has("json") {
        println!("{}", row.to_line());
    } else {
        // `--branches` tags a row that only lives on another branch, same as a list
        print!(
            "{}",
            branches::tag(core::render_full(log, &[row], 10_000), log, branch_of)
        );
    }
    Ok(())
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
fn quoted(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || "-_./:,@+=".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}
