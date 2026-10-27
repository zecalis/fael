//! `fael stats` numbers as a contract (`docs/stats.md`): parse `usage.jsonl`
//! text into a `Parsed`, join it with the repos' logs into one `Stats`, and
//! serialise that struct as-is for `--json` — the CLI, the desktop app and
//! any outside reader share the shape by construction. `day` cuts the same
//! inputs to one local day for `fael stats --day` and the desktop popover.
//!
//! Thin entry only: `parse` (text in, struct out), `aggregate` (plus the
//! `Stats` shape), `day` (plus the `DayView` shape), `state_dir` (the one
//! edge that reads the environment).

mod acted;
mod aggregate;
mod by_flag;
mod capture;
mod cross;
mod day;
mod days;
mod experience;
#[cfg(test)]
mod experience_tests;
mod flow;
mod friction;
mod leak;
mod lifecycle;
mod metrics;
mod outcomes;
mod overlap;
mod parse;
mod repeat;
mod retire;
mod said;
#[cfg(test)]
mod said_tests;
mod unused;
mod value;

pub use acted::ActedCounts;
pub use aggregate::{
    AskCount, Constants, Count, NonEnglish, Rounds, RowStatus, STATS_SCHEMA, Stats, TopRow,
    aggregate,
};
pub use by_flag::{FlagUsage, FlagUse};
pub use capture::Capture;
pub use cross::{CrossAgent, Reuse};
pub use day::{
    BUCKET_MIN, BUCKETS, Context, DAY_SCHEMA, DayPanels, DayView, Delivered, ForYou, Health,
    LastRow, Memory, RepoDay, STALE_DAYS, Timeline, day,
};
pub use days::{DayCount, days};
pub(crate) use experience::names_fix;
pub use experience::{
    CLOSE_TEMPLATE, Commit, Commits, Experience, FIX_CUTOFF_MS, Shape, bare_shas, cites_fix,
    close_shape, fael_cites, new_close, pr_cites, resolve,
};
pub use flow::{Flow, FlowMerge, FlowRun, Span, WeekCount};
pub use friction::{FIRST_CALL_WINDOW, Friction, SHAPE_GATE, Tally};
pub use leak::{LEAK_CALLS, LEAK_WINDOW_MS, Leak};
pub use lifecycle::{Lifecycle, States, lifecycle};
pub use metrics::ASK_ORDER;
pub use outcomes::{OUTCOMES_V, Pulled, RowOutcomes};
pub use overlap::{OVERLAP_WINDOW_MS, SameFile};
pub use parse::{Parsed, UsageRow, parse, session_stem, since, since_arg};
pub use repeat::{ContextLoop, context_loop};
pub use retire::{RETIRE_WINDOW_MS, Retired};
pub use said::{ASK_SPLIT, KINDS, KindYield};
pub use unused::{UNUSED_PUSHES, UnusedRow};
pub use value::{EventValue, Value};

/// The ask kind stored under `ask` in `usage.jsonl` — the vocabulary the
/// `asks` JSON map and the recording side share, so the two cannot drift.
pub const ASK_WARN: &str = "warning";

/// The usage event of a session-start that said nothing (PLAN-fael-say-gate
/// chunk 4c): the revert check counts it as a start, no injection total does.
pub const SILENT_START: &str = "silent-start";

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
