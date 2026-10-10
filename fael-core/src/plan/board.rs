//! One project's half of `fael board --json` (SPEC-fael-board §10, `"v": 1`): its plans with
//! per-state counts and its chunks with every derived field of §2 — deterministic math over
//! the stored rows and the clock. The cross-project lists (needs you, queue) are the CLI's.

use super::board_math::{clean, edges, plan_title};
use super::ready::{Ready, ready_in};
use super::store::{Store, err};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Serialize)]
pub struct Plan {
    pub app: String,
    pub name: String,
    pub title: String,
    pub area: String,
    pub kind: Option<String>,
    pub state: String,
    pub rank: Option<i64>,
    pub truth: String,
    pub source: String,
    pub spec: Option<String>,
    /// state → chunks in it; an archived (done/parked) plan lists only this
    pub counts: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct Chunk {
    pub uid: String,
    pub app: String,
    pub plan: String,
    pub label: Option<String>,
    pub title: String,
    pub state: String,
    pub size: Option<String>,
    pub model_hint: Option<String>,
    pub scope: Vec<String>,
    pub due: Option<String>,
    pub pin: Option<i64>,
    pub ready: bool,
    pub blocked_by: Vec<Blocker>,
    pub unblocks: usize,
    pub overlaps: Vec<String>,
    pub pair: Vec<String>,
    pub stalled: bool,
    pub ended: bool,
    pub wait: Option<Wait>,
    pub approval: Option<Approval>,
    pub run: Option<Run>,
    pub handoff: Option<Handoff>,
    /// plan rank — the queue's last tie-break before the chunk's place in its plan
    #[serde(skip)]
    pub rank: Option<i64>,
    #[serde(skip)]
    pub seq: i64,
}

#[derive(Debug, Serialize)]
pub struct Blocker {
    pub uid: Option<String>,
    #[serde(rename = "ref")]
    pub what: String,
}

#[derive(Debug, Serialize)]
pub struct Wait {
    pub on: Option<String>,
    pub text: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Approval {
    pub at: String,
    pub model: Option<String>,
    pub client: Option<String>,
}

/// The chunk's latest run (§7 fields, `id` = the start R).
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub id: String,
    pub client: Option<String>,
    pub session: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub pr: Option<i64>,
    pub out: Option<String>,
    pub started: String,
    pub last_seen: Option<String>,
    pub ended: Option<String>,
}

/// The agent's last word on the chunk: its `done` / `wait` / `after` text.
#[derive(Debug, Serialize)]
pub struct Handoff {
    pub at: String,
    pub text: String,
}

#[derive(Debug)]
pub struct Board {
    pub plans: Vec<Plan>,
    pub chunks: Vec<Chunk>,
}

impl Store {
    /// `today` = `YYYY-MM-DD` (dated data waits), `stale` = the RFC 3339 time 30 min ago:
    /// a live run last seen before it is `stalled` (a warning, never a state change).
    pub fn board(&self, today: &str, stale: &str) -> Result<Board, String> {
        let plans = self.board_plans()?;
        let truth: HashMap<String, bool> = plans
            .iter()
            .map(|p| (format!("{}/{}", p.app, p.name), p.truth == "db"))
            .collect();
        let runs = self.latest_runs()?;
        let handoffs = self.handoffs()?;
        let mut q = self
            .conn
            .prepare(
                "SELECT c.id, c.uid, p.app, p.name, c.label, c.title, c.state, c.size,
                        c.model_hint, c.scope, c.due, c.pin, c.wait_on, c.wait_text,
                        c.wait_until, c.approved_at, c.approved_model, c.approved_client,
                        p.rank, c.seq
                 FROM chunk c JOIN plan p ON p.id = c.plan
                 WHERE p.state NOT IN ('done', 'parked')
                 ORDER BY p.rank IS NULL, p.rank, p.app, p.name, c.seq",
            )
            .map_err(err)?;
        let rows = q
            .query_map([], |r| {
                let id: i64 = r.get(0)?;
                let uid: String = r.get(1)?;
                let label: Option<String> = r.get(4)?;
                let title: String = r.get(5)?;
                let state: String = r.get(6)?;
                let scope: Option<String> = r.get(9)?;
                let at: Option<String> = r.get(15)?;
                let waiting = state == "waiting";
                Ok((
                    id,
                    Chunk {
                        app: r.get(2)?,
                        plan: r.get(3)?,
                        title: clean(&title, label.as_deref()),
                        size: r.get(7)?,
                        model_hint: r.get(8)?,
                        scope: scope
                            .iter()
                            .flat_map(|s| s.lines())
                            .map(String::from)
                            .collect(),
                        due: r.get(10)?,
                        pin: r.get(11)?,
                        ready: false,
                        blocked_by: Vec::new(),
                        unblocks: 0,
                        overlaps: Vec::new(),
                        pair: Vec::new(),
                        stalled: false,
                        ended: false,
                        wait: waiting
                            .then(|| -> rusqlite::Result<Wait> {
                                Ok(Wait {
                                    on: r.get(12)?,
                                    text: r.get(13)?,
                                    until: r.get(14)?,
                                })
                            })
                            .transpose()?,
                        approval: at
                            .map(|at| -> rusqlite::Result<Approval> {
                                Ok(Approval {
                                    at,
                                    model: r.get(16)?,
                                    client: r.get(17)?,
                                })
                            })
                            .transpose()?,
                        run: runs.get(&id).cloned(),
                        handoff: None,
                        rank: r.get(18)?,
                        seq: r.get(19)?,
                        label,
                        state,
                        uid,
                    },
                ))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let mut handoffs = handoffs;
        let mut chunks = Vec::with_capacity(rows.len());
        let mut ids = Vec::with_capacity(rows.len());
        for (id, mut c) in rows {
            c.handoff = handoffs.remove(&c.uid);
            self.derive(id, &mut c, &truth, today, stale)?;
            ids.push(id);
            chunks.push(c);
        }
        edges(&self.conn, &ids, &mut chunks)?;
        Ok(Board { plans, chunks })
    }

    /// ready / blocked_by (the one `ready_in`), ended, stalled.
    fn derive(
        &self,
        id: i64,
        c: &mut Chunk,
        truth: &HashMap<String, bool>,
        today: &str,
        stale: &str,
    ) -> Result<(), String> {
        if matches!(c.state.as_str(), "open" | "waiting") {
            match ready_in(&self.conn, id, today)? {
                // `chunk start` takes only a db plan's chunk: an md one is never offered
                Ready::Yes => {
                    c.ready = truth
                        .get(&format!("{}/{}", c.app, c.plan))
                        .copied()
                        .unwrap_or(false);
                }
                Ready::Blocked(u) => {
                    c.blocked_by = u
                        .into_iter()
                        .map(|u| Blocker {
                            uid: u.uid,
                            what: u.what,
                        })
                        .collect();
                }
                Ready::No(_) => {}
            }
        }
        if c.state == "running"
            && let Some(r) = &c.run
        {
            c.ended = r.ended.is_some();
            let seen = r.last_seen.as_deref().unwrap_or(&r.started);
            c.stalled = !c.ended && seen < stale;
        }
        Ok(())
    }

    fn board_plans(&self) -> Result<Vec<Plan>, String> {
        let mut counts: HashMap<i64, BTreeMap<String, usize>> = HashMap::new();
        let mut q = self
            .conn
            .prepare("SELECT plan, state, COUNT(*) FROM chunk GROUP BY plan, state")
            .map_err(err)?;
        for row in q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(err)?
        {
            let (plan, state, n): (i64, String, usize) = row.map_err(err)?;
            counts.entry(plan).or_default().insert(state, n);
        }
        let mut q = self
            .conn
            .prepare(
                "SELECT id, app, name, title, area, kind, state, rank, truth, source, spec
                 FROM plan ORDER BY state IN ('done', 'parked'), rank IS NULL, rank, app, name",
            )
            .map_err(err)?;
        q.query_map([], |r| {
            let name: String = r.get(2)?;
            let title: String = r.get(3)?;
            Ok(Plan {
                counts: counts.remove(&r.get(0)?).unwrap_or_default(),
                app: r.get(1)?,
                title: plan_title(&title, &name),
                name,
                area: r.get(4)?,
                kind: r.get(5)?,
                state: r.get(6)?,
                rank: r.get(7)?,
                truth: r.get(8)?,
                source: r.get(9)?,
                spec: r.get(10)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }

    fn latest_runs(&self) -> Result<HashMap<i64, Run>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT chunk, start, client, session, model, worktree, branch, pr, out,
                        started, last_seen, ended
                 FROM run WHERE id IN (SELECT MAX(id) FROM run GROUP BY chunk)",
            )
            .map_err(err)?;
        q.query_map([], |r| {
            Ok((
                r.get(0)?,
                Run {
                    id: r.get(1)?,
                    client: r.get(2)?,
                    session: r.get(3)?,
                    model: r.get(4)?,
                    worktree: r.get(5)?,
                    branch: r.get(6)?,
                    pr: r.get(7)?,
                    out: r.get(8)?,
                    started: r.get(9)?,
                    last_seen: r.get(10)?,
                    ended: r.get(11)?,
                },
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }

    /// uid → the agent's latest `done` / `wait` / `after` text.
    fn handoffs(&self) -> Result<HashMap<String, Handoff>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT chunk_uid, at, text FROM event WHERE id IN (
                   SELECT MAX(id) FROM event
                   WHERE by = 'agent' AND to_state != 'running' AND text != ''
                   GROUP BY chunk_uid)",
            )
            .map_err(err)?;
        q.query_map([], |r| {
            Ok((
                r.get(0)?,
                Handoff {
                    at: r.get(1)?,
                    text: r.get(2)?,
                },
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
