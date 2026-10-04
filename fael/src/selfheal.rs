//! Self-heal on `add` (PLAN-fael-durable-log chunks 3b–e; reshaped by
//! PLAN-fael-selfheal-verdict chunk 1): fael answers from the log before it
//! asks the agent. (b) a repeated note on the same writer + branch + files
//! supersedes the open one itself; (c) a caller-supplied key is the stronger
//! identity — the single open row with the same kind + key + writer supersedes
//! too, except an `issue`, which is a finding rather than a topic: a key may
//! hold several, so an issue only replaces the same finding re-filed (same
//! words, a shared file); (d) a `Supersedes <id>` the flag left off sets it,
//! and a flag that resolves to nothing is rescued by the text when the text
//! names exactly one open row; (e) the one key these files already carry
//! becomes the row's key. The Stop-hook debt pattern (a row every turn,
//! nothing closing the old one) can no longer pile up, and a row filed where
//! a topic already lives carries that topic's identity. CLI and MCP share
//! `write::add_row`, so both behave the same.
//! Every automatic choice is reported in one info line — info, not a warning,
//! so it never counts as an ask. When several rows match or the rules
//! disagree, the row is filed, nothing is guessed, and one info line names
//! what was left open — never a reject, which would have no way out and fires
//! every Stop-hook turn on a branch already in debt.
//!
//! Fael doesn't try to understand everything. It makes only the decisions it
//! can justify, exposes the evidence when it can't, and makes every automatic
//! decision reversible.
//!
//! Proof or link: a row is hidden only on proof that the new one replaces it —
//! a text naming it, a flag, or the same kind + key from the same writer.
//! Shared files prove two notes related, not the same, so the files guess acts
//! only where no topic exists to lose (both notes keyless); a key on either
//! side, or any other overlap, files the row and names what stays open. A
//! silent hide costs the next reader a todo; a kept note costs one `close`.
//!
//! Chunk 3 (ActWarn + provenance): an act whose target's key differs from the
//! new row's is a cross-key act — only an explicit one (the text names the row)
//! can still reach it. The key is the weakest evidence, so `[selfheal]
//! cross_key` picks the exposure: `warn` (default) prints the act as one
//! `warning:` line, which the existing gate counts as an ask on CLI/MCP/batch;
//! `info` keeps the info line; `off` acts silently. Same-key acts stay info.
//! Every act stamps `decision_source` (`explicit:text`, `identity:key`,
//! `heuristic:files`, each with `:cross-key` when the key moved, `caller:flag`
//! for a resolving flag) so restore can trace an edge back to its cause; older
//! rows read as `unknown` and are never backfilled.
//!
//! Thin entry only — observation lives in `evidence` (Evidence, Candidate),
//! the Explicit > Identity > Heuristic policy table in `decide` (Eligibility,
//! Verdict), the byte-identical renderer in `render` (Heal). Chunk 2's five
//! invariants live as generated unit tests in `property`. Public paths never
//! change — callers keep `crate::selfheal::{evaluate, auto_key}`: `evaluate`
//! is the single Verdict source the write path, `add --dry-run` and MCP
//! `dry_run` share, so the three can never disagree on one input.

#[cfg(test)]
mod property;

mod decide;
mod evidence;
mod render;

pub(crate) use decide::{Evaluated, evaluate};
pub(crate) use evidence::auto_key;

/// Machine verdict for MCP `dry_run` and `add --dry-run --json`: names only,
/// never full structs — the verdict, its single target, the provenance
/// source, what `heal` would supersede, the target's evidence as enum names
/// (`files` is the shared count), and the info lines a real add would print.
pub(crate) fn verdict_json(ev: &Evaluated) -> serde_json::Value {
    let h = &ev.heal;
    serde_json::json!({
        "verdict": ev.verdict.name(),
        "target": ev.verdict.target(),
        "source": h.source,
        "supersedes": h.supersedes,
        "evidence": ev.evidence.as_ref().map(|e| serde_json::json!({
            "key": e.key.name(),
            "writer": e.writer.name(),
            "branch": e.branch.name(),
            "kind": e.kind.name(),
            "text": e.text.name(),
            "files": e.files.shared,
            "named": e.named.name(),
        })),
        "notes": h.notes,
    })
}

/// Dry-run stdout in one call: the JSON verdict with `--json`, else the
/// verdict line and the row the add would write — as `find` lists it, minus
/// the id (the real add mints its own).
pub(crate) fn verdict_text(
    ev: &Evaluated,
    json: bool,
    (log, row): (&fael_core::Log, &fael_core::Row),
) -> String {
    if json {
        return verdict_json(ev).to_string();
    }
    let line = fael_core::render(log, &[row], usize::MAX);
    let line = line.split_once("] ").map_or(line.as_str(), |(_, l)| l);
    format!("{}\nwould add: {}", verdict_line(ev), line.trim_end())
}

/// One stdout line for `add --dry-run`: the verdict `heal` would act on, with
/// source and full target id (paste-safe into `--supersedes`) — the notes
/// ride stderr exactly like a real add's warns.
pub(crate) fn verdict_line(ev: &Evaluated) -> String {
    let mut s = format!(
        "dry-run {} ({})",
        ev.verdict.name(),
        ev.heal.source.as_deref().unwrap_or("none")
    );
    if let Some(t) = ev.verdict.target() {
        s.push_str(&format!(" → {t}"));
    }
    s
}
