//! The MCP `add` tool — one row, `rows: [...]` batch, `dry_run` preview.
//! Split out of mcp.rs at the 400-line ratchet.

use super::args::{done, files, need, repo_for, s, urgent_ask};
use crate::hook::record_row_asks;
use crate::{Repo, write::AddOpts, write::add_row};
use serde_json::Value;

pub(super) fn add(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    // warnings are recorded per row where filed (with its id + session);
    // a preview costs no round — its lines are the answer, not an ask
    add_inner(a, &r).map(|(text, _)| text)
}

fn add_inner(a: &Value, r: &Repo) -> Result<(String, Vec<String>), String> {
    // `dry_run` previews the Verdict the real add would act on, writing
    // nothing — same build as a real add, the JSON carries verdict + evidence
    if a["dry_run"].as_bool().unwrap_or(false) && !a["rows"].is_array() {
        need(a, "kind")?;
        need(a, "text")?;
        let b = crate::batch::batch_row(a)?;
        let (p, _, _) = crate::write::prepare(r, &b.kind, &b.text, &b.files, b.opts)?;
        let v = crate::selfheal::verdict_json(&p.evaluated).to_string();
        return Ok((v, p.warns));
    }
    if a["dry_run"].as_bool().unwrap_or(false) {
        return Err("rejected: dry_run takes one row — drop rows: [...]".into());
    }
    // chunk 6b: `rows: [...]` files many rows in one call — each runs the same
    // validate + self-heal as a single add; a bad row reports alone, the rest save
    if let Some(rows) = a["rows"].as_array() {
        if rows.is_empty() {
            return Err("rejected: rows is empty — pass at least one row".into());
        }
        let mut out = vec![];
        let mut warns = vec![];
        let (mut failed, mut gated) = (0, 0);
        for (i, v) in rows.iter().enumerate() {
            let b = crate::batch::batch_row(v).map_err(|e| row_err(e, i))?;
            match add_row(r, &b.kind, &b.text, &b.files, b.opts) {
                Ok((row, _, w)) => {
                    record_row_asks("mcp", "mcp-add", &r.root, &row, &w);
                    out.push(format!("recorded {}", row.id));
                    out.extend(crate::batch::paste_line(&row));
                    out.extend(crate::batch::launch_line(&row));
                    // like a single add: each row's info/warning lines sit under its id
                    out.extend(w.iter().cloned());
                    // c2: held rows on this row's files, verdicts ready
                    let held = crate::hook::held_add_text(r, &row.files, &row.id);
                    if !held.is_empty() {
                        out.extend(held.lines().map(String::from));
                    }
                    warns.extend(w);
                }
                Err(e) => {
                    failed += 1;
                    gated += usize::from(e.starts_with(crate::batch::GATE_REJECT));
                    let e = format!("rejected: row {i}: {}", e.trim_start_matches("rejected: "));
                    out.push(e);
                }
            }
        }
        // like the CLI batch: any rejection turns the call into an error —
        // the saved rows stay saved, their warnings already sit under their ids
        // its first line says why, like the CLI batch's one reject
        if failed > 0 {
            out.insert(0, crate::batch::batch_reject(failed, gated, rows.len()));
            return Err(out.join("\n"));
        }
        // chunk 6e rides inside write::add_row — the saved ids are already seen
        return Ok((out.join("\n"), warns));
    }
    let (row, _, warns) = add_row(
        r,
        &need(a, "kind")?,
        &need(a, "text")?,
        &files(a),
        AddOpts {
            key: s(a, "key"),
            to: s(a, "to"),
            from: s(a, "from"),
            title: s(a, "title"),
            revisit: s(a, "revisit"),
            urgent: urgent_ask(a)?,
            supersedes: s(a, "supersedes"),
            force: a["force"].as_bool().unwrap_or(false),
        },
    )?;
    record_row_asks("mcp", "mcp-add", &r.root, &row, &warns);
    // the id stays alone on the first line, callers read it from there
    let mut text = done(&row.id, &[]);
    let routed = [
        crate::batch::paste_line(&row),
        crate::batch::launch_line(&row),
    ];
    for l in routed.iter().flatten().chain(&warns) {
        text.push('\n');
        text.push_str(l);
    }
    // c2: held rows on the new row's files, verdicts ready
    let held = crate::hook::held_add_text(r, &row.files, &row.id);
    if !held.is_empty() {
        text.push('\n');
        text.push_str(held.trim_end());
    }
    Ok((text, warns))
}

fn row_err(e: String, i: usize) -> String {
    format!("rejected: row {i}: {}", e.trim_start_matches("rejected: "))
}
