//! The MCP `chunk` tool — the CLI `chunk` verbs agents need without
//! switching screens: show, start, note, done, wait, after. One tool with a
//! `verb`, so the served schema stays small.

use super::args::{need, repo_for, s};
use crate::plan::db;
use fael_crew::{Here, Start, Store};
use serde_json::Value;

/// `chunk {"verb": ..., "uid": ..., "text": ...}`.
pub(super) fn chunk(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    let verb = need(a, "verb")?;
    let uid = need(a, "uid")?;
    let text = s(a, "text").unwrap_or_default();
    let now = fael_core::rfc3339(fael_core::now_ms());
    let wt = r.root.to_string_lossy().into_owned();
    let branch = crate::journal::head_branch(&r.root);
    let run = s(a, "run");
    let here = Here {
        worktree: &wt,
        branch: branch.as_deref(),
        now: &now,
        run: run.as_deref(),
    };
    let mut st = Store::open(&db(&r))?;
    match verb.as_str() {
        "show" => {
            let b = st.show(&uid)?;
            Ok(format!(
                "# {} ({})\nplan {} — {}\n\n{}\n",
                b.title, b.uid, b.plan, b.plan_title, b.brief
            )
            .trim_end()
            .to_string())
        }
        "note" => {
            if text.is_empty() {
                return Err("rejected: text is required for chunk note".into());
            }
            st.note(&uid, &text, &now)?;
            Ok(format!("chunk {uid} → noted for the next start"))
        }
        "start" => {
            let o = Start {
                run: run.clone(),
                force: a["force"].as_bool().unwrap_or(false),
                session: None,
                base: crate::git(&r.root, &["rev-parse", "HEAD"]),
                ..Start::default()
            };
            let started = st.start(&uid, &o, &here)?;
            Ok(started.text())
        }
        "done" => {
            if text.is_empty() {
                return Err("rejected: text is required for chunk done — the handoff".into());
            }
            st.done(&uid, &text, None, None, &here)?;
            Ok(format!("chunk {uid} → done"))
        }
        "wait" => {
            if text.is_empty() {
                return Err("rejected: text is required for chunk wait — the question".into());
            }
            let on = s(a, "on").unwrap_or_else(|| "owner".into());
            st.wait(&uid, &on, &text, None, &[], &here)?;
            Ok(format!("chunk {uid} → waiting on {on}"))
        }
        "after" => {
            if text.is_empty() {
                return Err("rejected: text is required for chunk after — why".into());
            }
            let other = s(a, "other")
                .ok_or("rejected: other is required for chunk after — the chunk waited on")?;
            st.after(&uid, &other, &text, &here)?;
            Ok(format!("chunk {uid} → after {other}"))
        }
        v => Err(format!(
            "rejected: unknown chunk verb {v} — show|start|note|done|wait|after"
        )),
    }
}
