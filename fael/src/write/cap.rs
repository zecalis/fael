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
    // only decisions count and a retired row is a note: no merge proof needed
    crate::hook::mark_dormant_files(r, log);
    // the same file under its old and new path counts once: both canonicalize
    // through renames (`fael mv` rows, git log -M). Folders and globs are not
    // folded in — they are not the same file.
    let al = crate::aliases::load(r, log, false);
    let canon = |f: &String| al.current(f).unwrap_or_else(|| f.clone());
    let open = core::find(log, &core::Filter::default());
    let ab = core::abbrev(log);
    // the flag may name a unique prefix (the lines below print those): resolve
    // it to the full id, or the row it names stays counted and the advised
    // re-file rejects again
    // c1: the canonical identity resolver (c2, F-IDENT) — never a fallback.
    let resolved = superseding.and_then(|s| match core::resolve_identity(log, s) {
        core::Ref::One(t) if !core::is_carrier_row(t) => Some(t.id.clone()),
        _ => None,
    });
    let superseding = resolved.as_deref().or(superseding);
    let mut lines = vec![];
    // the first capped file and one of its live ids: the `try` tail names the
    // exact re-file that passes, beside the `--force` escape
    let mut first: Option<(String, String)> = None;
    for f in row.files.iter().filter(|f| !crate::hook::is_anchor(f)) {
        let want = canon(f);
        let live: Vec<&&core::Row> = open
            .iter()
            .filter(|o| {
                o.kind == "decision"
                    && o.files.iter().any(|of| canon(of) == want)
                    && Some(o.id.as_str()) != superseding
            })
            .collect();
        if live.len() < MAX_LIVE {
            continue;
        }
        if first.is_none()
            && let Some(o) = live.first()
        {
            first = Some((f.clone(), ab.short(&o.id).to_string()));
        }
        lines.push(format!("{f} already has {} live decisions:", live.len()));
        for o in live {
            let id = ab.short(&o.id);
            lines.push(format!(
                "  {id} {} — re-file over it: --supersedes {id} · or retire it: fael close {id} \"<why>\"",
                o.display_title()
            ));
        }
        if is_doc(f) {
            lines.push(format!(
                "  or anchor it to a topic, not {f}: --files doc:<topic> (anchors are not capped)"
            ));
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    let try_tail = first.map_or(String::new(), |(f, id)| {
        format!(
            " — try `fael add decision \"<one topic>\" --title \"<≤15-word headline>\" --files {f} --supersedes {id}`"
        )
    });
    Err(format!(
        "rejected: nothing written — at most {MAX_LIVE} live decisions per file; supersede or merge into one\n{}\n(or add --force to file it as is){try_tail}",
        lines.join("\n")
    ))
}

/// A doc file (any `.md`, or under `docs/`) can carry its decision on a
/// `doc:<topic>` anchor instead, which has no disk and is not capped.
fn is_doc(f: &str) -> bool {
    f.ends_with(".md") || f.starts_with("docs/")
}
