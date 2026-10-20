//! The core read seam (crew-split): every hook, `find` and `refs` read
//! chunk data through here — never `fael_crew` directly. Opened read-only and
//! returning empty when `crew.db` (or its `fael.db` predecessor) or the plan
//! tables are missing, so core works with no crew installed.
//!
//! Thin entry only — the pure math lives in `pure` (verbs, overlap, shared,
//! rules, session files), the SQLite reads in `db` (crew owns the write side
//! and the migrations; this keeps its own read-only SQL so `fael-core` never
//! depends on `fael-crew`).

mod db;
mod pure;
mod ready;

/// A chunk's brief, as `chunk show` prints the fields core reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brief {
    pub uid: String,
    pub title: String,
    pub brief: String,
    pub plan: String,
    pub plan_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub id: i64,
    pub app: String,
    pub name: String,
    pub title: String,
    pub area: String,
    pub state: String,
    pub parent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StatsRun {
    pub started: String,
    pub ended: Option<String>,
    pub pr: Option<i64>,
    pub branch: Option<String>,
    pub chunk: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndedRun {
    pub plan: String,
    pub session: Option<String>,
    pub branch: Option<String>,
    pub ended: String,
}

pub use db::{
    all_plans, brief_row, chunk_brief, db_at, db_for, ended_runs, flow_runs, has_held, held,
    is_chunk, mark_seen, mark_seen_session, plan_by_name,
};
pub use pure::{
    CHUNK_VERBS, PLAN_VERBS, held_session_db, held_session_file, in_tmp, is_scratch, main_root_for,
    overlap, registered, rules, shared,
};
pub use ready::{first_ready, kickoff_text, ready_in};
