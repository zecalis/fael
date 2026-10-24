//! One project's half of `fael board --json` (SPEC-fael-board §10, `"v": 1`): its plans with
//! per-state counts and its chunks with every derived field of §2 — deterministic math over
//! the stored rows and the clock. The cross-project lists (needs you, queue) are the CLI's.

use super::Owner;
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
    pub state: String,
    /// always `db` (every plan's chunks live in the db): the app's board JSON v1 gates
    /// `chunk add --plan` on it
    pub truth: &'static str,
    /// the umbrella plan of the same app (`plan new --parent`, one level); absent = top level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// state → chunks in it; an archived (done/parked) plan lists only this
    pub counts: BTreeMap<String, usize>,
    /// the owner verbs fael takes for this plan now (park/unpark/done/undone/drop)
    pub owner_actions: Vec<&'static str>,
}

/// The owner verbs a plan in `state` takes: state moves the CLI accepts, plus
/// `drop` only when the plan holds no chunk and no child plan (the CLI still
/// guards running chunks and live children with a named reject).
pub fn plan_actions(
    state: &str,
    inbox: bool,
    has_chunks: bool,
    has_children: bool,
) -> Vec<&'static str> {
    if inbox {
        return Vec::new();
    }
    let mut out = match state {
        "active" | "blocked" => vec!["park", "done"],
        "parked" => vec!["unpark"],
        "done" => vec!["undone"],
        _ => Vec::new(),
    };
    if !has_chunks && !has_children {
        out.push("drop");
    }
    out
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
    /// the one model the launcher runs (`opencode run --model`): the latest run's,
    /// else the approval's, else the hint; null = none named one
    pub model: Option<String>,
    /// `chunk add|edit --client`; absent = none, `approve` then picks claude
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_hint: Option<String>,
    pub scope: Vec<String>,
    pub ready: bool,
    pub blocked_by: Vec<Blocker>,
    pub unblocks: usize,
    pub overlaps: Vec<String>,
    /// o1: the class per overlap pair, same order as `overlaps` (exact = same
    /// file, prefix = dir covers file); skipped from JSON until o4 serializes
    /// it — the golden pins chunk keys, so the shape stays byte-identical.
    #[serde(skip)]
    pub overlap_classes: Vec<(String, fael_core::OverlapClass)>,
    pub pair: Vec<String>,
    pub stalled: bool,
    pub ended: bool,
    pub wait: Option<Wait>,
    pub approval: Option<Approval>,
    pub run: Option<Run>,
    pub handoff: Option<Handoff>,
    /// the ledger issue ids it fixes (`chunk add --issue`)
    pub issues: Vec<String>,
    /// ids/uids on the `left:` line of the latest hand-off (`chunk done --left`); `[]` = none
    pub left: Vec<String>,
    /// closed-row warning lines for the title+brief (`chunk start` would refuse); `[]` = none.
    /// Filled by the CLI, only for chunks not done/dropped/replaced
    pub stale: Vec<String>,
    /// the owner verbs fael takes for this chunk now (`Owner::takes`)
    pub owner_actions: Vec<&'static str>,
    /// an open PR's branch merged into the base (`git merge-tree`, the CLI fills it): the
    /// conflicted paths, empty = merges clean; absent = not checked or git < 2.38
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicts: Option<Vec<String>>,
    /// the paths `done` claimed from the run's diff (`chunk.touched`); absent = never claimed.
    /// A wave PR's diff sliced per chunk, and what lanes started together really shared
    #[serde(skip_serializing_if = "Option::is_none")]
    pub touched: Option<Vec<String>>,
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
    /// the answers `chunk wait --option` offers, in order; `[]` = a free answer (01M4NWFS)
    pub options: Vec<String>,
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
    /// USD, as the client reported it to `fael run end R --cost`
    pub cost: Option<f64>,
    /// `chunk done --review clean|fixed|failed`; absent = none given
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
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

/// `chunk.wait_options`, a JSON array; NULL = none.
fn options(json: Option<String>) -> Vec<String> {
    json.and_then(|o| serde_json::from_str(&o).ok())
        .unwrap_or_default()
}

impl Store {
    /// `today` = `YYYY-MM-DD` (dated data waits), `stale` = the RFC 3339 time 30 min ago:
    /// a live run last seen before it is `stalled` (a warning, never a state change).
    pub fn board(&self, today: &str, stale: &str) -> Result<Board, String> {
        let plans = self.board_plans()?;
        let runs = self.latest_runs()?;
        let handoffs = self.handoffs()?;
        let mut q = self
            .conn
            .prepare(
                "SELECT c.id, c.uid, p.app, p.name, c.label, c.title, c.state, c.size,
                        c.model_hint, c.scope, c.wait_on, c.wait_text, c.wait_until,
                        c.approved_at, c.approved_model, c.approved_client, c.seq, c.wait_options,
                        c.touched, c.client_hint
                 FROM chunk c JOIN plan p ON p.id = c.plan
                 WHERE p.state NOT IN ('done', 'parked')
                 ORDER BY p.app, p.name, c.seq",
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
                let at: Option<String> = r.get(13)?;
                let waiting = state == "waiting";
                Ok((
                    id,
                    Chunk {
                        app: r.get(2)?,
                        plan: r.get(3)?,
                        title: clean(&title, label.as_deref()),
                        size: r.get(7)?,
                        model_hint: r.get(8)?,
                        model: None, // derive() resolves run → approval → hint
                        client_hint: r.get(19)?,
                        scope: scope
                            .iter()
                            .flat_map(|s| s.lines())
                            .map(String::from)
                            .collect(),
                        ready: false,
                        blocked_by: Vec::new(),
                        unblocks: 0,
                        overlaps: Vec::new(),
                        overlap_classes: Vec::new(),
                        pair: Vec::new(),
                        stalled: false,
                        ended: false,
                        wait: waiting
                            .then(|| -> rusqlite::Result<Wait> {
                                Ok(Wait {
                                    on: r.get(10)?,
                                    text: r.get(11)?,
                                    until: r.get(12)?,
                                    options: options(r.get(17)?),
                                })
                            })
                            .transpose()?,
                        approval: at
                            .map(|at| -> rusqlite::Result<Approval> {
                                Ok(Approval {
                                    at,
                                    model: r.get(14)?,
                                    client: r.get(15)?,
                                })
                            })
                            .transpose()?,
                        run: runs.get(&id).cloned(),
                        handoff: None,
                        issues: Vec::new(),
                        left: Vec::new(),
                        stale: Vec::new(),
                        owner_actions: Owner::actions(&state),
                        conflicts: None,
                        touched: r
                            .get::<_, Option<String>>(18)?
                            .map(|t| t.lines().map(String::from).collect()),
                        seq: r.get(16)?,
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
            c.issues = super::issue::of(&self.conn, id)?;
            self.derive(id, &mut c, today, stale)?;
            ids.push(id);
            chunks.push(c);
        }
        edges(&self.conn, &ids, &mut chunks)?;
        Ok(Board { plans, chunks })
    }

    /// ready / blocked_by (the one `ready_in`), ended, stalled.
    fn derive(&self, id: i64, c: &mut Chunk, today: &str, stale: &str) -> Result<(), String> {
        // the launcher runs one model: the latest run's, else the approval's, else the hint
        c.model = c
            .run
            .as_ref()
            .and_then(|r| r.model.clone())
            .or_else(|| c.approval.as_ref().and_then(|a| a.model.clone()))
            .or_else(|| c.model_hint.clone());
        if matches!(c.state.as_str(), "open" | "waiting") {
            match ready_in(&self.conn, id, today)? {
                Ready::Yes => c.ready = true,
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
        type PlanRow = (i64, String, String, String, String, String, Option<String>);
        let mut counts: HashMap<i64, BTreeMap<String, usize>> = HashMap::new();
        let mut q = self
            .conn
            .prepare("SELECT plan, state, COUNT(*) FROM chunk GROUP BY plan, state")
            .map_err(err)?;
        for row in q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(err)?
        {
            let (plan, state, n): (i64, String, i64) = row.map_err(err)?;
            counts.entry(plan).or_default().insert(state, n as usize);
        }
        let mut q = self
            .conn
            .prepare(
                "SELECT id, app, name, title, area, state, parent
                 FROM plan ORDER BY state IN ('done', 'parked'), app, name",
            )
            .map_err(err)?;
        let rows: Vec<PlanRow> = q
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let parents: std::collections::HashSet<(String, String)> = rows
            .iter()
            .filter_map(|(_, app, _, _, _, _, parent)| parent.clone().map(|u| (app.clone(), u)))
            .collect();
        rows.into_iter()
            .map(|(id, app, name, title, area, state, parent)| {
                let has_chunks = counts
                    .get(&id)
                    .is_some_and(|c| c.values().sum::<usize>() > 0);
                let has_children = parents.contains(&(app.clone(), name.clone()));
                let inbox = app.is_empty() && name == "inbox";
                Ok(Plan {
                    counts: counts.remove(&id).unwrap_or_default(),
                    owner_actions: plan_actions(&state, inbox, has_chunks, has_children),
                    app,
                    title: plan_title(&title, &name),
                    name,
                    area,
                    state,
                    parent,
                    truth: "db",
                })
            })
            .collect::<Result<_, _>>()
            .map_err(err)
    }

    fn latest_runs(&self) -> Result<HashMap<i64, Run>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT chunk, start, client, session, model, worktree, branch, pr, out,
                        started, last_seen, ended, cost, review
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
                    cost: r.get(12)?,
                    review: r.get(13)?,
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
