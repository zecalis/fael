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
    // a plan doc shows its open work: rows of chunks the doc ticked leave, and
    // without --limit the page is PLAN_KICKOFF_ROWS (the cut line names the rest)
    let plan = files
        .first()
        .and_then(|f| core::plan_anchor(f, &r.cfg.anchor_prefixes).map(|p| (f, p)));
    let page_limit = limit.or(plan.as_ref().map(|_| core::PLAN_KICKOFF_ROWS));
    let f = Filter {
        files: al.expand_all(&files),
        limit,
        offset,
        ..Filter::default()
    };
    // kickoff ranks the full set itself, so it pages after — same helper as query()
    let mut ranked = core::kickoff(&log, &f, &r.root, &al, &r.cfg.anchor_prefixes);
    if let Some((file, anchor)) = &plan
        && let Ok(doc) = std::fs::read_to_string(r.root.join(file))
    {
        ranked = core::drop_closed(ranked, anchor, &core::closed_chunks(&doc));
    }
    let (rows, total) = core::page(ranked, page_limit, offset);
    // a tag whose branch is already in HEAD says so: its handoff may be stale
    let branch_of = super::merged::mark(&r.root, branch_of, &rows);
    // a handoff whose code files moved since it was written says so too: the
    // same tag channel, but the note's own plan file never counts (it moves
    // every chunk, so counting it would label every handoff)
    let branch_of =
        super::merged::mark_changed(&r.root, branch_of, &rows, &al, &r.cfg.anchor_prefixes);
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
