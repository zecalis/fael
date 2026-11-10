//! The empty answer: why nothing matched, plus the nearest rows when the
//! caller can use them. `--json` keeps stdout exact — one JSON object with
//! `rows: []` and the hint/why — so a script parses stdout alone.

use super::misses;
use crate::core::{self, Filter, Log};

/// Regex metachars are literal AND here: hint `--key` glob or two finds.
/// Stderr only, so `--json` stdout stays exact.
pub(crate) fn emit_hint(a: &crate::Args, f: &Filter) {
    if !a.has("json")
        && let Some(h) = core::regex_hint(f.text.as_deref())
    {
        eprintln!("fael: {h}");
    }
}

/// An empty find: the nearest rows under their label, else why nothing
/// matched. `--json` prints one object on stdout (`rows: []` plus the
/// hint/why) and stays silent on stderr, so a script reads stdout alone.
pub(crate) fn nearest(
    a: &crate::Args,
    root: &std::path::Path,
    log: &Log,
    f: &Filter,
    budget: usize,
    branch_of: &crate::journal::BranchMap,
) {
    let (head, near) = misses::explain(root, "cli", log, f, "--files");
    if a.has("json") {
        let hint = match core::regex_hint(f.text.as_deref()) {
            Some(h) => format!("{head}\n{h}"),
            None => head,
        };
        println!(
            "{}",
            serde_json::json!({"rows": [], "hint": hint, "why": hint})
        );
        return;
    }
    // regex metachars are literal here (`all_words` is AND): hint `--key` glob
    // or two finds, same text as MCP.
    let head = match core::regex_hint(f.text.as_deref()) {
        Some(h) => format!("{head}\n{h}"),
        _ => head,
    };
    if near.is_empty() {
        eprintln!("fael: {head}");
        return;
    }
    let full = a.has("full");
    let out = match full {
        true => core::render_full(log, &near, budget),
        false => core::render(log, &near, budget),
    };
    let out = crate::journal::tag(out, branch_of);
    println!("{head}");
    print!("{out}");
    let text = format!("{head}\n{out}");
    let n = out.lines().filter(|l| l.starts_with("- [")).count();
    let shown: Vec<String> = near.iter().take(n).map(|r| r.id.clone()).collect();
    super::super::hook::record_found(
        "cli",
        "find",
        root,
        &text,
        &shown,
        (f.key.as_deref(), &f.files, None),
    );
}
