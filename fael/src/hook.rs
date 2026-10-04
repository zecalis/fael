//! `fael hook <stop|session-start|read|edit|search|prompt> [--client c]` — stdin in, stdout out.
//! The decision (`core::decide_stop`, `core::push`) is written once; each
//! adapter only parses its client's JSON and renders the answer back.
//! No `--client` = the neutral protocol from SPEC §9: Event in, Reply out.
//! Adapters: `claude`, `codex` (same hook shape; codex hands stop its last
//! message and edits as apply_patch). OpenCode's plugin speaks neutral.
//!
//! Thin entry only — the events live in `hook/`:
//! `protocol` (neutral Event + shared ctx), `say` (the one door to the agent's
//! context: Reply, Outbox, Kind and its noise policy), `claude` (client adapters),
//! `stop` (turn-end work/bug rule), `capture` (the reply's `fael <kind>:` lines), `session` (session-start kickoff),
//! `push` (read/edit context), `counts` (its count lines), `search` (files a grep/glob/shell read touched), `prompt` (the open-key pointer on a user prompt), `focus` (session Focus written at start and
//! read by the push), `state` (per-machine session files),
//! `autosync` (session-start and turn-end `fael sync`, off-switch `[sync] auto`),
//! `tally` (the user channel's per-session ledger: reminders, receipt),
//! `usage` (SPEC §8 accounting + `stats`), `asks` (chunk-3a ask types +
//! real tokens), `markers` (bug phrases).
//!
//! The hook always exits 0. Any internal error is an empty Reply (let the
//! turn through) — a memory tool must never break the agent's tool call.

mod also;
mod asks;
mod askstats;
mod autosync;
mod capture;
mod changed;
mod claude;
mod counts;
mod focus;
mod markers;
mod prompt;
mod protocol;
mod push;
mod say;
#[cfg(test)]
mod say_contract;
mod search;
mod session;
mod state;
mod stats_text;
mod stop;
mod tally;
mod usage;

pub(crate) use asks::{
    ASK_REJECT, ASK_WARN, record_asks, record_cli_reject, record_mcp, record_row_asks,
};
pub(crate) use protocol::cmd;
pub(crate) use usage::{aggregate, load, record_found, stats};

// What the binary shares: `write` reads session edits and checks anchors,
// `maintain` asks where the gitignore rule comes from.
pub(crate) use push::is_anchor;
pub(crate) use session::{deliberate, ignore_source};
pub(crate) use state::{Edit, note_seen, now_rfc3339, session_edits, state_dir};
pub(crate) use tally::{note_closed, note_filed};
