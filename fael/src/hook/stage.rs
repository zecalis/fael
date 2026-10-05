//! A repo's push-gate stage under `push_policy = auto` (PLAN-fael-learn-loop
//! chunk 6, SPEC §D): `shadow → canary → ramp`, or `baseline` after a rollback.
//! The push reads the resolved stage from one small state file beside the log
//! (`cache/push-gate.json`); it never reads the log or the usage. The evaluator
//! runs at session start and Stop — never on the push path — and only when the
//! repo's search pushes since its last look reach `EVAL_EVERY`. A change of
//! stage is filed as a `policy:push-gate` decision row first, and the state
//! file moves only if that row was written: a policy changes through its row.
//! State missing, torn, or for another policy version = `shadow`, never a guess.
// ponytail: a stage change lands in the file while other sessions of the repo
// may be mid-way — a session can change arm once, at a promotion (canary →
// ramp only adds candidates, a rollback only removes them). Pin the arm in the
// session's seen file if that ever pollutes an arm.

use super::protocol::Event;
use crate::core::{self, Stage};
use crate::{journal, repo_at, write};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const FILE: &str = "push-gate.json";
/// Search pushes of a repo since the last look before the evaluator looks again.
const EVAL_EVERY: usize = 100;
/// The row a stage change is filed under; its title is what a brief shows.
pub(super) const KEY: &str = "policy:push-gate";

#[derive(Serialize, Deserialize, Default)]
struct State {
    policy: String,
    stage: String,
    /// Bytes of `usage.jsonl` already counted — the next look reads only past it.
    usage_at: u64,
    /// Search pushes of this repo counted since the last evaluation.
    #[serde(default)]
    pending: usize,
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

fn save(file: &Path, s: &State) {
    let Some(dir) = file.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!(".push-gate-{}.tmp", std::process::id()));
    if std::fs::write(&tmp, serde_json::to_string(s).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, file);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The session's stop event: its repo, resolved like `autosync` does.
pub(crate) fn evaluate_for(e: &Event) {
    let cwd = e
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if let Some(repo) = cwd.and_then(|c| repo_at(&c).ok()) {
        evaluate(&repo);
    }
}

/// Look at this repo's evidence once `EVAL_EVERY` new search pushes are in, and
/// move its stage by the table in `core::next_stage`. Fail open throughout: a
/// look that cannot finish changes nothing.
pub(crate) fn evaluate(repo: &crate::Repo) {
    if repo.cfg.push_policy != core::AUTO || journal::home(repo).is_none() {
        return;
    }
    let file = path(&repo.root);
    let mut st = load(&file);
    let from = stage_of(&st);
    if from == Stage::Baseline {
        return; // a rollback is final for this version
    }
    let me = journal::scope(&repo.root.to_string_lossy());
    let (n, end) = new_pushes(&me, st.usage_at);
    st.pending += n;
    if st.pending < EVAL_EVERY {
        // count once, not again at every Stop: the next look starts at `end`
        if n > 0 {
            (st.policy, st.stage, st.usage_at) = (core::TOUCH.name(), from.name().into(), end);
            save(&file, &st);
        }
        return;
    }
    let memo = RefCell::new(HashMap::<String, String>::new());
    let scope = |r: &str| {
        let mut m = memo.borrow_mut();
        m.entry(r.to_string())
            .or_insert_with(|| journal::scope(r))
            .clone()
    };
    let u = super::usage::load_where(None, &|r| scope(r) == me);
    let t = core::stats::tune(
        &u.parsed,
        &u.logs,
        super::usage::local_tz_offset_min(),
        &scope,
    );
    let (verdict, evidence) = match from {
        Stage::Shadow => (core::stats::shadow_verdict(&t.all), shadow_evidence(&t)),
        _ => match t.validation.iter().find(|v| v.repo == me) {
            Some(v) => (v.verdict.clone(), arms_evidence(v)),
            None => (
                core::stats::Verdict::insufficient("no arm has a search push yet"),
                Value::Null,
            ),
        },
    };
    let to = core::next_stage(from, verdict.result);
    if to != from && file_change(repo, from, to, &verdict, evidence).is_err() {
        return; // no row, no change: the next look tries again
    }
    st = State {
        policy: core::TOUCH.name(),
        stage: to.name().into(),
        usage_at: end,
        pending: 0,
    };
    save(&file, &st);
}

/// Search pushes of this repo past byte `from` of the usage log, and where the
/// log ends now. A log shorter than `from` was pruned: count from its start.
fn new_pushes(me: &str, from: u64) -> (usize, u64) {
    let Ok(mut f) = std::fs::File::open(super::state::state_dir().join("usage.jsonl")) else {
        return (0, from);
    };
    let from = match f.metadata() {
        Ok(m) if m.len() >= from => from,
        _ => 0,
    };
    let mut tail = Vec::new();
    if f.seek(SeekFrom::Start(from)).is_err() || f.read_to_end(&mut tail).is_err() {
        return (0, from);
    }
    let mut mine: HashMap<String, bool> = HashMap::new();
    let n = String::from_utf8_lossy(&tail)
        .lines()
        .filter(|l| l.contains("\"event\":\"search\""))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| {
            v["repo"].as_str().is_some_and(|r| {
                *mine
                    .entry(r.to_string())
                    .or_insert_with(|| journal::scope(r) == me)
            })
        })
        .count();
    (n, from + tail.len() as u64)
}

fn shadow_evidence(t: &core::stats::Tune) -> Value {
    json!({"sizes": t.all.sizes, "replay": t.all.policies.iter().find(|p| p.policy == core::TOUCH.name())})
}

/// The numbers the verdict read, small enough to live in a row.
fn arms_evidence(v: &core::stats::Validation) -> Value {
    json!({
        "strata": v.strata.iter().map(|s| json!({"client": s.client, "status": s.status})).collect::<Vec<_>>(),
        "exposure_cut_pct": v.bars.exposure_cut_pct,
        "retained": v.bars.retained,
        "missed_push": v.candidate_all.missed_push,
        "retrieved_pct": v.bars.retrieved_pct,
        "dup_pct": v.bars.dup_pct,
    })
}

/// File the `policy:push-gate` decision row (SPEC §D body).
fn file_change(
    repo: &crate::Repo,
    from: Stage,
    to: Stage,
    verdict: &core::stats::Verdict,
    validation: Value,
) -> Result<(), String> {
    let pct = to.candidate_pct();
    let body = json!({
        "policy": core::TOUCH.name(),
        "repo": repo.root.file_name().map(|n| n.to_string_lossy()),
        "from": from.name(),
        "to": to.name(),
        "arm_split": {"candidate": pct.unwrap_or(0), "baseline": pct.map_or(100, |p| 100 - p)},
        "reason": if verdict.why.is_empty() { verdict.result.to_string() } else { verdict.why.join("; ") },
        "validation": validation,
    });
    let opts = write::AddOpts {
        key: Some(KEY.into()),
        to: None,
        title: Some(format!(
            "push-gate {}: {} → {}",
            core::TOUCH.name(),
            from.name(),
            to.name()
        )),
        revisit: None,
        urgent: core::Urgent::Unset,
        supersedes: None,
        force: false,
    };
    write::add_row(repo, "decision", &body.to_string(), &[KEY.into()], opts).map(|_| ())
}
