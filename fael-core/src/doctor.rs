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
//! `scan` (read-only check) and `fix` (repairs under the lock).

mod fix;
mod scan;

use std::path::PathBuf;

pub use fix::{current_month, fix};
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
}

impl Problem {
    fn error(kind: Kind, fixable: bool, file: Option<PathBuf>, detail: String) -> Problem {
        Problem {
            kind,
            severity: Severity::Error,
            fixable,
            file,
            detail,
        }
    }

    fn info(kind: Kind, detail: String) -> Problem {
        Problem {
            kind,
            severity: Severity::Info,
            fixable: false,
            file: None,
            detail,
        }
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
