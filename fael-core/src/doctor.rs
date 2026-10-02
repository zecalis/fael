//! `fael doctor [--fix]` — self-healing per SPEC §11.
//!
//! Reading never fails and `doctor` without `--fix` only reports (exit 1 when
//! an `Error` is present, so CI can gate on it). `--fix` repairs what it can —
//! broken lines and torn tails move to `.fael/quarantine/` (never deleted),
//! conflict markers are stripped, encoding is normalised, the
//! `merge=union` line is added — every repair through tmp + rename under the
//! `.fael/.lock`. What `--fix` cannot repair (duplicates → `compact`; legacy
//! rows without `files` → never invent files) stays report-only.
//!
//! Thin entry only — the report types live here, the verbs in `doctor/`:
//! `scan` (read-only check), `fix` (repairs under the lock) and `precision`
//! (per-rule self-heal precision from restore labels, over a `Log`).

mod fix;
mod precision;
mod scan;

use std::path::PathBuf;

pub use fix::{current_month, fix};
pub use precision::precision;
pub use scan::scan;

/// Only `Error` fails `doctor`; `Info` is reported and never fails `--fix`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Error,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Broken,
    Torn,
    Conflict,
    Union,
    Duplicate,
    Encoding,
    NoFiles,
    MultiFael,
    Ignored,
    NoLog,
    /// `store = "local"`: rows live in the clone's journal, not the tree
    Local,
    Future,
    Oversize,
    /// open rows whose files all no longer exist — they never push again
    Gone,
    /// open rows that still name a file that no longer exists — they push,
    /// but likely describe the repo as it was
    PartGone,
    /// open rows whose text names a backticked path with no file behind it —
    /// a dead pointer the next reader follows (`files[]` rot stays Gone's)
    Stale,
    /// rows hidden only by a `supersedes` marker whose newest version is
    /// already closed — the trap a pre-chain-close binary leaves when it closes
    /// the newest version alone: the old versions stay hidden with no close row
    /// and no command reached them. Closing the newest version now closes the
    /// chain, so this reports legacy rows; `fael close <id>` on each repairs
    /// them (the relaxed guard lets it through once the head is closed).
    Superseded,
    /// open rows — or close reasons — whose text cites an id-shaped token
    /// with no row behind it: a dead citation the next reader takes as
    /// confirmation (`files[]` rot stays Gone's, backticked paths stay
    /// Stale's; ambiguous prefixes resolve, so they never count)
    Phantom,
    /// open rows filed on a branch whose PR was closed without merge — the
    /// work likely died with the branch; judged by the `gh` CLI in `doctor`,
    /// never in core (core never spawns processes)
    Orphan,
    /// local branches whose PR already merged but still exist — found by the
    /// other session's branch the hard way (row-hygiene chunk 9); judged by
    /// `gh` in `doctor`, never in core
    Merged,
    /// open rows chunk 3 would have warned about at add time (no key, several
    /// topics, or long text) — the agent skipped the warning, so `doctor`
    /// repeats it (row-hygiene chunk 10); judged by `fat_reasons`, never new logic
    Fat,
    /// open notes filed on a branch whose PR merged after the row was born —
    /// the work landed, so the note is stale (durable-log chunk 2); judged by
    /// branch name (squash/​rebase drop the sha), never by sha. The row's birth
    /// comes from its ULID time, the merge time from `mergedAt`.
    Shipped,
    /// like `Shipped` but the merge time is unknown (only `git branch
    /// --merged` says the branch landed, no `mergedAt`) — shown as
    /// `[Shipped?]` so the reader confirms before closing.
    ShippedMaybe,
    /// open rows with a letter outside every accepted `[lang] rows` script —
    /// the add-time warning the agent skipped, repeated here so one translate
    /// batch can supersede them all (PLAN-fael-languages chunk 2); judged by
    /// `lang::row_language_check`, never new logic
    NotEnglish,
    /// open rows whose files took many commits after the row was written —
    /// the code moved on, so the row may restate or contradict it; counted
    /// by `git log` in `doctor`, never in core (core never spawns processes)
    Drifted,
    /// per-rule self-heal precision from restore labels
    /// (PLAN-fael-selfheal-restore chunk 3): a `restores` row labels the edge
    /// it reverts as wrong, an explicit re-supersede after it as right;
    /// re-adds the healer files alone never label, edges never restored are
    /// not counted, pre-verdict edges (no `decision_source`) are skipped.
    /// Info-only, shown only when at least one label lands.
    Precision,
    /// client wiring (hooks, plugin, skill) behind this binary — `fael install`
    /// would still change something, so a hook that shipped later (sub-agent
    /// reply capture) stays off until `fael upgrade`; judged by the install
    /// dry pass in `doctor`, never in core (core never reads client configs).
    /// Info-only: it is machine state, not the repo's log.
    Wiring,
    /// rows that have not reached their destination: uncommitted `.fael/log`
    /// files under `store = "tracked"`, or this writer's rows not on the
    /// remote after a failed `fael sync` (PLAN-fael-local-first chunk 2);
    /// judged in the adapter (git, sync state), never in core. Info-only.
    Late,
    /// a row (open or closed) whose serialised line trips `validate::secret`
    /// — the ingress checks (add, import, sync ingest) prevent this, so what
    /// lands here predates them or was hand-edited in. Detect-only by design:
    /// never `--fix`ed, the owner rotates the token and then `fael purge`s the
    /// row. Names the label and location, never the token.
    Secret,
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub kind: Kind,
    pub severity: Severity,
    /// true when `--fix` repairs this (everything else is report-only).
    pub fixable: bool,
    /// The log file this is about, when there is one — `--fix` groups
    /// content repairs by it instead of parsing `detail`.
    pub file: Option<PathBuf>,
    pub detail: String,
    /// Full row ids this problem is about, in the same order as the examples
    /// in `detail` — empty when the problem is not about rows. `--json`
    /// prints them so a cleanup agent can act (`fael close <id>`) without
    /// re-deriving the detector outside fael (01M3M2ZV).
    pub ids: Vec<String>,
    /// `--fix` close actions for the adapter: `(row id, close text)`. Only the
    /// confirmed `[Shipped]` notes fill this — `[Shipped?]` never does, and
    /// core never closes a row itself (the adapter owns the `gh` evidence).
    pub closes: Vec<(String, String)>,
}

impl Problem {
    fn error(kind: Kind, fixable: bool, file: Option<PathBuf>, detail: String) -> Problem {
        Problem {
            kind,
            severity: Severity::Error,
            fixable,
            file,
            detail,
            ids: vec![],
            closes: vec![],
        }
    }

    pub fn info(kind: Kind, detail: String) -> Problem {
        Problem {
            kind,
            severity: Severity::Info,
            fixable: false,
            file: None,
            detail,
            ids: vec![],
            closes: vec![],
        }
    }

    /// Attach the full row ids `detail` only sketches (`--json` prints them).
    pub fn with_ids(mut self, ids: Vec<String>) -> Problem {
        self.ids = ids;
        self
    }

    /// Attach the mechanical close actions `--fix` may take (`[Shipped]`).
    /// A problem with close actions is by definition `fixable` (the adapter
    /// applies them), and its `ids` are the rows being closed.
    pub fn with_closes(mut self, closes: Vec<(String, String)>) -> Problem {
        self.fixable = !closes.is_empty();
        self.ids = closes.iter().map(|(id, _)| id.clone()).collect();
        self.closes = closes;
        self
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub problems: Vec<Problem>,
}

impl Report {
    /// Anything that fails `doctor` (exit 1) — and what `--fix` must clear.
    pub fn errors(&self) -> impl Iterator<Item = &Problem> {
        self.problems
            .iter()
            .filter(|p| p.severity == Severity::Error)
    }
}
