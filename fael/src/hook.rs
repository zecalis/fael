//! `fael hook <stop|session-start|read|edit> [--client c]` — stdin in, stdout out.
//! The decision (`core::decide_stop`, `core::push`) is written once; each
//! adapter only parses its client's JSON and renders the answer back.
//! No `--client` = the neutral protocol from SPEC §9: Event in, Reply out.
//! Adapters: `claude`, `codex` (same hook shape; codex hands stop its last
//! message and edits as apply_patch). OpenCode's plugin speaks neutral.
//!
//! Thin entry only — the events live in `hook/`:
//! `protocol` (neutral Event/Reply + shared ctx), `claude` (client adapters),
//! `stop` (turn-end work/bug rule), `capture` (the reply's `fael <kind>:` lines), `session` (session-start kickoff),
//! `push` (read/edit context), `search` (files a Grep/Bash read touched), `focus` (session Focus written at start and
//! read by the push), `state` (per-machine session files),
//! `autosync` (session-end `fael sync`, off-switch `[sync] auto`),
//! `usage` (SPEC §8 accounting + `stats`), `asks` (chunk-3a ask types +
//! real tokens), `markers` (bug phrases).
//!
//! The hook always exits 0. Any internal error is an empty Reply (let the
//! turn through) — a memory tool must never break the agent's tool call.

mod asks;
mod askstats;
mod autosync;
mod capture;
mod claude;
mod focus;
mod markers;
mod protocol;
mod push;
mod search;
mod session;
mod state;
mod stop;
mod usage;

pub(crate) use asks::{ASK_REJECT, ASK_WARN, record_asks, record_cli_reject, record_mcp};
pub(crate) use protocol::cmd;
pub(crate) use usage::stats;

// What the binary shares: `write` reads session edits and checks anchors,
// `maintain` asks where the gitignore rule comes from.
pub(crate) use push::is_anchor;
pub(crate) use session::{deliberate, ignore_source};
pub(crate) use state::{Edit, note_seen, session_edits, state_dir};
