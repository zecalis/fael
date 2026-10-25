//! `fael hook <stop|session-start|read|edit|search|prompt> [--client c]` — stdin in, stdout out.
//! The reply (`stop::decide`, `core::push`) is built once — stop never blocks; each
//! adapter only parses its client's JSON and renders the answer back.
//! No `--client` = the neutral protocol from SPEC §9: Event in, Reply out.
//! Adapters: `claude`, `codex` (same hook shape; codex hands stop its last
//! message and edits as apply_patch). OpenCode's plugin speaks neutral.
//!
//! Thin entry only — the events live in `hook/`:
//! `protocol` (neutral Event + shared ctx), `say` (the one door to the agent's
//! context: Reply, Outbox, Kind and its noise policy), `claude` (client adapters),
//! `opencode` (OpenCode normalization over neutral: the plugin speaks neutral,
//! the adapter shapes V1-vs-V2 payloads before the same arms run),
//! `stop` (turn-end work/bug rule), `cited` (the `cited` outcome: a said id typed into a tool input or reply), `session` (session-start kickoff), `named` (open issues a commit on the branch names, at session start),
//! `push` (read/edit context), `search` (files a grep/glob/shell read touched), `prompt` (the open-key pointer on a user prompt), `focus` (session Focus written at start and
//! read by the push), `state` (per-machine session files),
//! `autosync` (session-start and turn-end `fael sync`, off-switch `[sync] auto`),
//! `tally` (the user channel's per-session ledger: reminders, receipt),
//! `usage` (SPEC §8 accounting + `stats`), `asks` (chunk-3a ask types +
//! real tokens), `markers` (bug phrases).
//!
//! The hook always exits 0. Any internal error is an empty Reply (let the
//! turn through) — a memory tool must never break the agent's tool call.

mod also;
mod asked;
mod asks;
mod askstats;
mod autosync;
mod changed;
mod check;
mod cited;
mod claude;
mod codes;
mod decision;
mod experience_text;
mod flag_usage;
mod flow;
mod focus;
mod friction;
mod held;
mod held_evidence;
mod held_rotate;
mod in_context;
mod lifecycle;
mod markers;
mod opencode;
mod overlap;
mod prompt;
mod protocol;
mod push;
mod retired;
mod review;
mod say;
#[cfg(test)]
mod say_budget;
#[cfg(test)]
mod say_contract;
mod search;
mod session;
mod shape;
mod spent_notes;
mod state;
mod stats_text;
mod stop;
mod tally;
mod usage;
pub(crate) mod usage_files;

pub(crate) use asks::{ASK_WARN, record_asks, record_row_asks};
pub(crate) use friction::{note_empty, record_cli, record_mcp_call};
pub(crate) use held::{
    add_text as held_add_text, done_text as held_text, shown_for as held_shown,
    start_text as held_start,
};
pub(crate) use protocol::cmd;
pub(crate) use usage::{aggregate, load, record_found, stats};

// What the binary shares: `write` reads session edits and checks anchors,
// `maintain` asks where the gitignore rule comes from.
pub(crate) use changed::{PUSH_MAX_BYTES, files_changed, mark_dormant, mark_dormant_files};
pub(crate) use fael_core::is_anchor;
pub(crate) use state::{Edit, note_seen, now_rfc3339, session_edits, started_path, state_dir};
pub(crate) use tally::{note_closed, note_filed};
