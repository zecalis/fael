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
//! Thin entry only — observation lives in `evidence` (Evidence, Candidate),
//! the Explicit > Identity > Heuristic policy table in `decide` (Eligibility,
//! Verdict), the byte-identical renderer in `render` (Heal). Chunk 2's five
//! invariants live as generated unit tests in `property`. Public paths never
//! change — callers keep `crate::selfheal::{heal, auto_key}`.

#[cfg(test)]
mod property;

mod decide;
mod evidence;
mod render;

pub(crate) use decide::heal;
pub(crate) use evidence::auto_key;
