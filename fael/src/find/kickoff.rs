//! `fael kickoff <file|PLAN>`: the rows to start a session on. Split out of
//! find.rs at the 400-line ratchet.

use super::{show, working_or_branches};
use crate::{Args, aliases};
use fael_core::{self as core, Filter};

pub(crate) fn kickoff(a: &Args, anchor: Option<&String>) -> Result<(), String> {
    let r = crate::repo()?;
    let files = core::normalize_files(&Vec::from_iter(anchor.cloned()), &r.cwd, &r.root)?;
    let (base, jtags) = crate::journal::read(&r);
    let (log, branch_of) = working_or_branches(a, base, jtags, &r);
    let al = aliases::load(&r, &log, true);
    let (limit, offset) = a.paging()?;
    let f = Filter {
        files: al.expand_all(&files),
        limit,
        offset,
        ..Filter::default()
    };
    // kickoff ranks the full set itself, so it pages after — same helper as query()
    let (rows, total) = core::page(
        core::kickoff(&log, &f, &r.root, &al, &r.cfg.anchor_prefixes),
        limit,
        offset,
    );
    let base = a.page_base("kickoff", anchor.map(String::as_str), limit);
    let shown = show(
        a,
        a.has("full"),
        &log,
        &rows,
        // an explicit --limit wins over the token budget, same as find
        limit.map_or(r.cfg.kickoff_tokens, |_| usize::MAX),
        core::Cut {
            total,
            offset,
            next: &|n| format!("{base} --offset {n}"),
        },
        &branch_of,
    )?;
    let none = (None, &files[..], None);
    crate::hook::record_found("cli", "kickoff", &r.root, &shown, none);
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
