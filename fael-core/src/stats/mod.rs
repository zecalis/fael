//! `fael stats` numbers as a contract (`docs/stats.md`): parse `usage.jsonl`
//! text into a `Parsed`, join it with the repos' logs into one `Stats`, and
//! serialise that struct as-is for `--json` — the CLI, the desktop app and
//! any outside reader share the shape by construction. `day` cuts the same
//! inputs to one local day for `fael stats --day` and the desktop popover.
//!
//! Thin entry only: `parse` (text in, struct out), `aggregate` (plus the
//! `Stats` shape), `day` (plus the `DayView` shape), `state_dir` (the one
//! edge that reads the environment).

mod aggregate;
mod capture;
mod cross;
mod day;
mod metrics;
mod parse;
mod retire;
mod value;

pub use aggregate::{
    AskCount, BlockOutcome, Constants, Count, NonEnglish, RealAvg, Rounds, RowStatus, STATS_SCHEMA,
    Stats, TopRow, aggregate,
};
pub use capture::Capture;
pub use cross::{CrossAgent, Reuse};
pub use day::{
    BUCKET_MIN, BUCKETS, Context, DAY_SCHEMA, DayPanels, DayView, Delivered, ForYou, Health,
    LastRow, Memory, RepoDay, STALE_DAYS, Timeline, day,
};
pub use metrics::ASK_ORDER;
pub use parse::{Parsed, StopBlock, UsageRow, parse, since, since_arg};
pub use retire::{RETIRE_WINDOW_MS, Retired};
pub use value::{EventValue, Value};

/// Ask kinds stored under `ask` in `usage.jsonl` — the vocabulary the `asks`
/// JSON map and the recording side share, so the two cannot drift.
pub const ASK_REJECT: &str = "reject";
pub const ASK_BLOCK: &str = "stop-block";
pub const ASK_WARN: &str = "warning";

/// Per-machine runtime state, never in `.fael/`. `FAEL_STATE_DIR` wins (tests
/// and scratch runs); otherwise the home state dir. The one function that
/// reads the environment, called by the CLI and the desktop app alike.
pub fn state_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("FAEL_STATE_DIR")
        && !d.is_empty()
    {
        return std::path::PathBuf::from(d);
    }
    home()
        .unwrap_or_else(|| ".".into())
        .join(".local/state/fael")
}

/// The user's home: `HOME` first (git and Git Bash on Windows honour it too,
/// and tests set it), else the OS answer (`USERPROFILE` on Windows).
fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(std::env::home_dir)
}
