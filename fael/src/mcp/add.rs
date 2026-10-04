//! The MCP `add` tool — one row, `rows: [...]` batch, `dry_run` preview.
//! Split out of mcp.rs at the 400-line ratchet.

use super::args::{done, files, need, repo_for, s, urgent_ask};
use crate::hook::{ASK_REJECT, record_mcp, record_row_asks};
use crate::{Repo, write::AddOpts, write::add_row};
use serde_json::Value;

pub(super) fn add(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    match add_inner(a, &r) {
        Err(e) => {
            record_mcp(&r.root, "mcp-add", ASK_REJECT, &e);
            Err(e)
        }
        // warnings are recorded per row where filed (with its id + session);
        // a preview costs no round — its lines are the answer, not an ask
        Ok((text, _)) => Ok(text),
    }
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
        let mut failed = 0;
        for (i, v) in rows.iter().enumerate() {
            let b = crate::batch::batch_row(v).map_err(|e| row_err(e, i))?;
            match add_row(
                r,
                &b.kind,
                &b.text,
                &b.files,
                AddOpts {
                    key: b.opts.key,
                    to: b.opts.to,
                    title: b.opts.title,
                    revisit: b.opts.revisit,
                    urgent: b.opts.urgent,
                    supersedes: b.opts.supersedes,
                    force: b.opts.force,
                },
            ) {
                Ok((row, _, w)) => {
                    record_row_asks("mcp", "mcp-add", &r.root, &row, &w);
                    out.push(format!("recorded {}", row.id));
                    out.extend(crate::batch::paste_line(&row));
                    out.extend(crate::batch::launch_line(&row));
                    // like a single add: each row's info/warning lines sit under its id
                    out.extend(w.iter().cloned());
                    warns.extend(w);
                }
                Err(e) => {
                    failed += 1;
                    let e = format!("rejected: row {i}: {}", e.trim_start_matches("rejected: "));
                    record_mcp(&r.root, "mcp-add", ASK_REJECT, &e);
                    out.push(e);
                }
            }
        }
        // like the CLI batch: any rejection turns the call into an error —
        // the saved rows stay saved, their warnings already sit under their ids
        if failed > 0 {
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
    Ok((text, warns))
}

fn row_err(e: String, i: usize) -> String {
    format!("rejected: row {i}: {}", e.trim_start_matches("rejected: "))
}
