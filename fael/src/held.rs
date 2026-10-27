//! `fael held start|done` (hidden): the text `fael-crew` prints in the middle of `chunk start`
//! and `chunk done` — contracts and the open rows on the chunk's files, with their verdicts.
//! It needs this binary's session state and said-once outbox, so fael-crew asks (FAEL_BIN)
//! instead of copying it. Files arrive one per line on stdin; stdout is the finished text.

use crate::{core, hook};
use std::collections::HashSet;
use std::io::Read;
use std::process::ExitCode;

pub(crate) fn cmd(kind: &str, since: Option<String>) -> Result<ExitCode, String> {
    let r = crate::repo()?;
    let mut raw = String::new();
    std::io::stdin()
        .read_to_string(&mut raw)
        .map_err(|e| format!("held: stdin: {e}"))?;
    let files: Vec<String> = raw.lines().map(String::from).collect();
    match kind {
        "start" => {
            contracts(&r, &files);
            print!("{}", hook::held_start(&r, &files));
        }
        "done" => {
            let since = since.as_deref().and_then(core::ts_ms);
            print!("{}", hook::held_text(&r, &files, since));
            print!("{}", hook::done_loop_line(&r).unwrap_or_default())
        }
        _ => return Err(format!("held: unknown kind {kind:?} — start|done")),
    }
    Ok(ExitCode::SUCCESS)
}

/// Each open `contract:*` row whose files overlap a started chunk's scope (the board's
/// prefix rule), as `id title` + body after the brief; none → no heading. fael never builds
/// a contract or a scope from brief text.
fn contracts(r: &crate::Repo, scope: &[String]) {
    if scope.is_empty() {
        return;
    }
    let mut log = crate::read(r);
    hook::mark_dormant(r, &mut log);
    let hide = core::closed(&log);
    let hide: HashSet<&str> = hide.union(&core::superseded(&log)).copied().collect();
    let rows: Vec<_> = log
        .rows
        .iter()
        .filter(|w| w.key.as_deref().is_some_and(|k| k.starts_with("contract:")))
        .filter(|w| !hide.contains(w.id.as_str()) && !log.dormant.contains(&w.id))
        .filter(|w| core::overlap(&w.files, scope))
        .collect();
    if rows.is_empty() {
        return;
    }
    println!("\ncontracts:");
    for w in rows {
        let title = w
            .title
            .as_deref()
            .map(|t| format!("{t}\n"))
            .unwrap_or_default();
        println!(
            "> {}",
            format!("{} {title}{}", w.id, w.text).replace('\n', "\n> ")
        );
    }
}
