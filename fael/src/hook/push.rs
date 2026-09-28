//! The read/edit push: resolve renames through the L1 cache only (no git
//! spawn on this path), record the edit, cap the push at `budget.push_rows`,
//! and say each row once per session.

use super::asks::hook_meta;
use super::protocol::{Event, Reply, ctx};
use super::state::{edits_path, record_edits, seen_path, take_risk};
use super::usage::record_usage;
use crate::{aliases, core};
use std::collections::HashSet;

/// The stashed Weak-signal line: one line, shown on the next push only.
fn risk_line(marker: &str, files: &[String]) -> String {
    format!(
        "fael note: this session mentioned a possible problem (\"{marker}\") — file an issue if it holds up: fael add issue \"<what is at risk>\" --files {}",
        files.join(",")
    )
}

/// The omitted line (§7): what the cap and budget cut, plus the exact next
/// call. Render's own budget line stays for find — the push swaps its cut
/// line for this one.
fn omitted_line(omitted: usize, files: &[String]) -> String {
    let what = if files.len() == 1 {
        "this file"
    } else {
        "these files"
    };
    format!(
        "… +{omitted} more about {what} — fael find --files {}",
        crate::find::quoted(&files.join(","))
    )
}

/// Render the selected rows with the token budget — the hard cap after the
/// row cap — swapping render's budget cut line for the omitted line: what
/// the cap and the budget cut, plus the exact next call (render itself is
/// untouched, so find keeps its own cut line). Returns the tagged body and
/// how many rows were actually said (usage counts only those).
fn cut_body(
    body: String,
    shown: usize,
    omitted_select: usize,
    files: &[String],
    tags: &crate::find::branches::BranchMap,
) -> (String, usize) {
    let n = body.lines().filter(|l| l.starts_with("- [")).count();
    let omitted = omitted_select + shown.saturating_sub(n);
    let mut lines: Vec<String> = body
        .lines()
        .filter(|l| !l.starts_with("… +"))
        .map(str::to_string)
        .collect();
    if omitted > 0 {
        lines.push(omitted_line(omitted, files));
    }
    (crate::find::branches::tag(lines.join("\n") + "\n", tags), n)
}

/// A `scheme:ref` anchor (opaque, never a filesystem path) — the same rule
/// core uses: a scheme of ≥ 2 lower chars before the first `:`.
pub(crate) fn is_anchor(f: &str) -> bool {
    if f.contains("://") || !f.contains(':') {
        return false;
    }
    // cheap anchor check without reaching into core: scheme of ≥2 lower chars
    let scheme = f.split(':').next().unwrap_or("");
    scheme.len() >= 2
        && scheme.as_bytes()[0].is_ascii_lowercase()
        && scheme
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'+' | b'.' | b'-'))
}

pub(crate) fn push(e: &Event, event: &str) -> Reply {
    let no = || Reply {
        block: false,
        reason: None,
        context: None,
    };
    let c = match ctx(e) {
        Some(c) => c,
        None => return no(),
    };
    // normalize through core — outside the repo falls away, never errors out
    let mut files = vec![];
    for f in &e.files {
        // the client sends whatever the OS gave it (`/var/…` vs `/private/var/…`);
        // resolve symlinks while the repo root is already resolved, or the
        // lexical strip in normalize_files reads the file as outside the repo
        // (anchors are opaque, never filesystem paths)
        let f = if is_anchor(f) {
            f.clone()
        } else {
            std::fs::canonicalize(c.repo.cwd.join(f)) // join keeps an absolute f
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| f.clone())
        };
        if let Ok(mut n) =
            core::normalize_files(std::slice::from_ref(&f), &c.repo.cwd, &c.repo.root)
        {
            files.append(&mut n);
        }
    }
    if files.is_empty() {
        return no();
    }
    // only adopted repos — stop never blocks without a log anyway
    if event == "edit" && !c.session.is_empty() && c.repo.fael.join("log").is_dir() {
        record_edits(
            &edits_path(&c.session, &c.repo.root),
            &c.repo.root.to_string_lossy(),
            &c.session,
            &files,
        );
    }
    // L1 gather, then L3/L4 rank + select — `Focus::default()` is today's
    // order, capped (chunk 2 reads the session Focus here). The read/edit
    // push resolves renames through the L1 cache only — no git spawn on
    // this path (one spawn is ~9 ms against a 5 ms ceiling).
    // `session-start` refreshes the cache once per session instead.
    // reads skip the same-directory tier (chunk 2: the noisiest tier — rows
    // about neighbouring files); edits keep it, a module decision matters
    // most while changing that module.
    let policy = core::PushPolicy {
        max_rows: c.repo.cfg.push_rows,
        budget: c.repo.cfg.push_tokens,
        background: core::PUSH_BACKGROUND,
    };
    let mut tiered = core::push_tiered(
        &c.log,
        &files,
        &aliases::load(&c.repo, &c.log, false),
        event == "read",
    );
    // a row already pushed this session is still in the agent's context — say it once
    let seen = (!c.session.is_empty()).then(|| seen_path(&c.session, &c.repo.root));
    if let Some(p) = &seen {
        let old = std::fs::read_to_string(p).unwrap_or_default();
        let old: HashSet<&str> = old.lines().collect();
        tiered.retain(|(r, _)| !old.contains(r.id.as_str()));
    }
    let sel = core::select(tiered, &core::Focus::default(), &policy);
    let rows = sel.shown;
    // a stashed Weak risk is taken here — shown once, whether or not rows join it
    let risk = (!c.session.is_empty())
        .then(|| take_risk(&c.session, &c.repo.root))
        .flatten();
    if rows.is_empty() {
        // a stashed risk still gets its one line, even with no rows to join
        if let Some(marker) = risk {
            let context = risk_line(&marker, &files);
            let meta = hook_meta(&c.session, None, true);
            record_usage(&c.client, event, &c.repo.root, &context, &[], &meta);
            return Reply {
                block: false,
                reason: None,
                context: Some(context),
            };
        }
        return no();
    }
    let (body, n) = cut_body(
        core::render(&c.log, &rows, policy.budget),
        rows.len(),
        sel.omitted,
        &files,
        &c.tags,
    );
    // usage counts only what was actually said — ids cut off never reached
    // any context, so stats must not count them
    let shown: Vec<String> = rows.iter().take(n).map(|r| r.id.clone()).collect();
    if let Some(p) = &seen {
        // only what fit the budget was said; the cut rows may push on a later read
        let out: String = shown.iter().map(|id| format!("{id}\n")).collect();
        use std::io::Write;
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(&c.repo.root));
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .and_then(|mut f| f.write_all(out.as_bytes()));
    }
    let context = format!("fael mem for {}:\n{body}", files.join(", "));
    let context = match risk {
        Some(marker) => format!("{context}\n{}", risk_line(&marker, &files)),
        None => context,
    };
    let meta = hook_meta(&c.session, None, true);
    record_usage(&c.client, event, &c.repo.root, &context, &shown, &meta);
    Reply {
        block: false,
        reason: None,
        context: Some(context),
    }
}
