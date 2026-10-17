//! Decision 01M5528X: at most `MAX_LIVE` live decisions per file. A 4th must
//! supersede or merge with one of them, so noise does not grow with writers
//! times rows. Dormant and closed rows do not count; the row this add
//! supersedes does not either (it is leaving).

use crate::core;

pub(crate) const MAX_LIVE: usize = 3;

/// `Err` (the reject, with ready `--supersedes` and close commands) when a
/// real file of `row` already holds `MAX_LIVE` live decisions. Anchors have no
/// disk to compare, so they are not capped.
pub(crate) fn check(
    r: &crate::Repo,
    log: &mut core::Log,
    row: &core::Row,
    superseding: Option<&str>,
) -> Result<(), String> {
    if row.kind != "decision" {
        return Ok(());
    }
    crate::hook::mark_dormant(r, log);
    let open = core::find(log, &core::Filter::default());
    let ab = core::abbrev(log);
    let mut lines = vec![];
    for f in row.files.iter().filter(|f| !crate::hook::is_anchor(f)) {
        let live: Vec<&&core::Row> = open
            .iter()
            .filter(|o| {
                o.kind == "decision" && o.files.contains(f) && Some(o.id.as_str()) != superseding
            })
            .collect();
        if live.len() < MAX_LIVE {
            continue;
        }
        lines.push(format!("{f} already has {} live decisions:", live.len()));
        for o in live {
            let id = ab.short(&o.id);
            lines.push(format!(
                "  {id} {} — re-file over it: --supersedes {id} · or retire it: fael close {id} \"<why>\"",
                o.display_title()
            ));
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    Err(format!(
        "rejected: nothing written — at most {MAX_LIVE} live decisions per file; supersede or merge into one\n{}\n(or add --force to file it as is)",
        lines.join("\n")
    ))
}
