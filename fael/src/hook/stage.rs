//! A repo's push-gate stage under `push_policy = auto` (PLAN-fael-learn-loop
//! chunk 6, SPEC §D): `shadow → canary → ramp`, or `baseline` after a rollback,
//! read from one small state file beside the log (`cache/push-gate.json`).
//! The gate only ever cut search pushes, and a search no longer pushes
//! (decision `say:push-at-edit`), so the Stop evaluator that moved the stage
//! and filed `policy:push-gate` rows is gone; `tune` still reads the stage.
//! State missing, torn, or for another policy version = `shadow`, never a guess.
// ponytail: a stage change lands in the file while other sessions of the repo
// may be mid-way — a session can change arm once, at a promotion (canary →
// ramp only adds candidates, a rollback only removes them). Pin the arm in the
// session's seen file if that ever pollutes an arm.

use crate::core::{self, Stage};
use crate::journal;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "push-gate.json";
/// The row a stage change is filed under; its title is what a brief shows.
pub(super) const KEY: &str = "policy:push-gate";

#[derive(Serialize, Deserialize, Default)]
struct State {
    policy: String,
    stage: String,
    /// Bytes of `usage.jsonl` already counted — the next look reads only past it.
    usage_at: u64,
    /// The live usage file's month at that look :
    /// an archive named from it on is that file, moved since.
    #[serde(default)]
    usage_month: String,
    /// Search pushes of this repo counted since the last evaluation.
    #[serde(default)]
    pending: usize,
    /// Where the current canary or ramp stage began — the live file's byte and
    /// month at the look that moved it (as `usage_at`/`usage_month`). A look in
    /// that stage reads usage from here, not from this and last month. Empty
    /// month = no offset (a state from before chunk 7, or shadow): read as before.
    #[serde(default)]
    stage_at: u64,
    #[serde(default)]
    stage_month: String,
}

/// `<repo scope>/cache/push-gate.json`: the journal all worktrees of a clone
/// share, else the folder's own `.fael/` — the same repo identity `tune` uses.
fn path(root: &Path) -> PathBuf {
    journal::root(root)
        .unwrap_or_else(|| root.join(".fael"))
        .join("cache")
        .join(FILE)
}

fn load(file: &Path) -> State {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn stage_of(s: &State) -> Stage {
    match Stage::parse(&s.stage) {
        Some(g) if s.policy == core::TOUCH.name() => g,
        _ => Stage::Shadow,
    }
}

/// The stage a push of this worktree runs in.
pub(crate) fn current(root: &Path) -> Stage {
    stage_of(&load(&path(root)))
}

/// The stage of a `tune` repo scope (the journal root, or a plain folder).
pub(crate) fn of_scope(scope: &str) -> Stage {
    let dir = Path::new(scope);
    [dir.join("cache"), dir.join(".fael").join("cache")]
        .iter()
        .map(|d| d.join(FILE))
        .find(|f| f.is_file())
        .map_or(Stage::Shadow, |f| stage_of(&load(&f)))
}
