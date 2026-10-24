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
    /// joint start (01M5PE238): the live run's mode and partner chunk uid.
    /// Skipped from JSON — the board's overlap math reads it, the app's
    /// contract stays byte-identical (overlap_classes is skipped the same way).
    #[serde(skip)]
    pub joint: Option<String>,
    /// joint start partner (see `joint`); skipped from JSON likewise.
    #[serde(skip)]
    pub partner: Option<String>,
}

/// The last word on the chunk: the agent's `done` / `wait` / `after` text,
/// or — when the chunk is dropped or parked — the owner's close text
/// (`chunk drop|park "<reason>"`), so the reason survives in JSON.
#[derive(Debug, Serialize)]
pub struct Handoff {
    pub at: String,
    pub text: String,
}

/// Cost-per-feature read (run-telemetry t5): what share of runs carry the
/// client-reported attribution t2+t3 write, and where the money went.
/// Old rows stay NULL (never backfilled); every cost number rides with its
/// unknown-% so a reader never mistakes a partial sum for a total.
/// The `session` on each `Run` is the usage join key where joinable.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Models {
    /// every run row in crew.db, oldest first for the window below
    pub total: usize,
    /// runs with non-null client AND model (the t5 model share numerator)
    pub with_client_model: usize,
    /// runs with cost > 0 (the t5 cost share numerator; a zero-cost run
    /// reads as unknown for the metric — it doesn't demonstrate attribution)
    pub with_cost: usize,
    /// 0-100, one decimal: runs lacking client+model / total (0 when empty)
    pub unknown_model_pct: f64,
    /// 0-100, one decimal: runs without cost > 0 / total (0 when empty)
    pub unknown_cost_pct: f64,
    /// `client/model` (missing part reads `unknown`) → runs and summed USD;
    /// cost ≤ 0 adds 0 to the sum but counts in `unknown_cost_pct`
    pub by_model: BTreeMap<String, ModelRow>,
    /// the agreed verdict window (Decision 2026-10-24, kept 80/50/30)
    pub window: CostWindow,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ModelRow {
    pub runs: usize,
    pub cost: f64,
    pub unknown_cost_pct: f64,
    /// runs in this group lacking cost (exact merge key; never serialized)
    #[serde(skip)]
    pub missing: usize,
}

/// First 30 agent-started runs at/after the t2+t3 ship: model share >= 80
/// and opencode cost share >= 50 passes; either outcome keeps the code.
/// `pass: None` = fewer than 30 runs so far (pending, not failing).
#[derive(Debug, Default, Clone, Serialize)]
pub struct CostWindow {
    pub n: usize,
    pub need: usize,
    pub model_pct: Option<f64>,
    pub cost_pct: Option<f64>,
    pub pass: Option<bool>,
}

#[derive(Debug)]
pub struct Board {
    pub plans: Vec<Plan>,
    pub chunks: Vec<Chunk>,
    pub models: Models,
}

impl Models {
    /// Merge per-repo summaries into the board-wide read (exact: totals sum,
    /// `unknown_%` recomputes from the sums, `by_model` merges runs/cost and
    /// recomputes from the kept `missing`). The window keeps the leading
    /// repo's (largest `n`): the verdict window is per crew.db, and the board
    /// merges repos it never joins.
    pub fn merge(all: &[Models]) -> Models {
        let mut out = Models::default();
        for m in all {
            out.total += m.total;
            out.with_client_model += m.with_client_model;
            out.with_cost += m.with_cost;
            for (k, r) in &m.by_model {
                let e = out.by_model.entry(k.clone()).or_default();
                e.runs += r.runs;
                e.cost += r.cost;
                e.missing += r.missing;
            }
            if m.window.n > out.window.n {
                out.window = m.window.clone();
            }
        }
        out.unknown_model_pct = pct(out.total - out.with_client_model, out.total);
        out.unknown_cost_pct = pct(out.total - out.with_cost, out.total);
        for e in out.by_model.values_mut() {
            e.unknown_cost_pct = pct(e.missing, e.runs);
        }
        out.window.need = NEED;
        out
    }
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
        let (handoffs, closes) = (self.handoffs()?, self.closes()?);
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
        let mut closes = closes;
        let (mut chunks, ids) = self.finish(rows, &mut handoffs, &mut closes, today, stale)?;
        edges(&self.conn, &ids, &mut chunks)?;
        Ok(Board {
            plans,
            chunks,
            models: self.models()?,
        })
    }

    /// ready / blocked_by (the one `ready_in`), ended, stalled.
    fn finish(
        &self,
        rows: Vec<(i64, Chunk)>,
        handoffs: &mut HashMap<String, Handoff>,
        closes: &mut HashMap<String, Handoff>,
        today: &str,
        stale: &str,
    ) -> Result<(Vec<Chunk>, Vec<i64>), String> {
        let mut chunks = Vec::with_capacity(rows.len());
        let mut ids = Vec::with_capacity(rows.len());
        for (id, mut c) in rows {
            c.handoff = handoff_for(handoffs, closes, &c);
            c.issues = super::issue::of(&self.conn, id)?;
            self.derive(id, &mut c, today, stale)?;
            ids.push(id);
            chunks.push(c);
        }
        Ok((chunks, ids))
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
                        started, last_seen, ended, cost, review, joint, partner
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
                    joint: r.get(14)?,
                    partner: r.get(15)?,
                },
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }

    /// Cost-per-feature read (t5): aggregate every run row (oldest first)
    /// into totals, per-model sums and the 30-run verdict window.
    /// Pure over the rows: NULL stays NULL, percentages round to 1 decimal.
    fn models(&self) -> Result<Models, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT client, model, cost, started, worktree
                 FROM run ORDER BY started",
            )
            .map_err(err)?;
        let rows: Vec<ModelRowIn> = q
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        Ok(summarize(&rows))
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

    /// uid → the owner's latest `drop` / `park` reason. Read beside `handoffs` so a
    /// dropped or parked chunk's `handoff` carries its reason instead of null.
    fn closes(&self) -> Result<HashMap<String, Handoff>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT chunk_uid, at, text FROM event WHERE id IN (
                   SELECT MAX(id) FROM event
                   WHERE by = 'owner' AND to_state IN ('dropped', 'parked')
                     AND text IS NOT NULL AND text != ''
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

/// One run row for the Models read: (client, model, cost, started, worktree).
type ModelRowIn = (
    Option<String>,
    Option<String>,
    Option<f64>,
    String,
    Option<String>,
);
/// t2+t3 ship (PRs #646 opencode cost, #649 CLI session): the verdict
/// window counts agent-started runs at/after this stamp.
const TELEMETRY_SHIP: &str = "2026-10-24T12:21:00Z";
/// Agreed bar (Decision 2026-10-24, kept 80/50/30): model share over the
/// window, cost share over opencode runs in it, window size.
const NEED: usize = 30;
const MODEL_BAR: f64 = 80.0;
const COST_BAR: f64 = 50.0;

/// Unknown-%: 0-100 with one decimal (0 when empty, never NaN).
fn pct(missing: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (missing as f64 * 1000.0 / total as f64).round() / 10.0
}

/// Known share: 0-100 with one decimal (None when empty = unmeasurable).
fn share(with: usize, total: usize) -> Option<f64> {
    if total == 0 {
        return None;
    }
    Some(((with as f64 * 1000.0 / total as f64).round() / 10.0).min(100.0))
}

fn is_opencode(client: &Option<String>, model: &Option<String>) -> bool {
    client.as_deref().is_some_and(|c| c.starts_with("opencode"))
        || model.as_deref().is_some_and(|m| m.starts_with("opencode"))
}

/// Pure aggregation over `(client, model, cost, started, worktree)` rows,
/// oldest first. Agent-started = a worktree is recorded (every `chunk
/// start` binds one; board-launched launcher rows may lack it).
fn summarize(rows: &[ModelRowIn]) -> Models {
    let total = rows.len();
    let with_client_model = rows
        .iter()
        .filter(|(c, m, _, _, _)| c.is_some() && m.is_some())
        .count();
    let with_cost = rows
        .iter()
        .filter(|(_, _, k, _, _)| k.as_ref().is_some_and(|k| *k > 0.0))
        .count();
    let mut by_model: BTreeMap<String, ModelRow> = BTreeMap::new();
    let mut missing_by_key: HashMap<String, usize> = HashMap::new();
    for (client, model, cost, _, _) in rows {
        let key = format!(
            "{}/{}",
            client.as_deref().unwrap_or("unknown"),
            model.as_deref().unwrap_or("unknown")
        );
        let e = by_model.entry(key.clone()).or_default();
        e.runs += 1;
        if cost.as_ref().is_some_and(|k| *k > 0.0) {
            e.cost += (*cost).unwrap_or(0.0);
        } else {
            *missing_by_key.entry(key).or_default() += 1;
        }
    }
    for (key, e) in &mut by_model {
        let missing = *missing_by_key.get(key).unwrap_or(&0);
        e.missing = missing;
        e.unknown_cost_pct = pct(missing, e.runs);
    }
    let windowed: Vec<&ModelRowIn> = rows
        .iter()
        .filter(|(_, _, _, started, worktree)| {
            worktree.is_some() && started.as_str() >= TELEMETRY_SHIP
        })
        .take(NEED)
        .collect();
    let n = windowed.len();
    let (model_pct, cost_pct, pass) = if n < NEED {
        (None, None, None)
    } else {
        let with = windowed
            .iter()
            .filter(|(c, m, _, _, _)| c.is_some() && m.is_some())
            .count();
        let model_pct = share(with, n);
        let opencode_n = windowed
            .iter()
            .filter(|(c, m, _, _, _)| is_opencode(c, m))
            .count();
        let opencode_with = windowed
            .iter()
            .filter(|(c, m, k, _, _)| is_opencode(c, m) && k.as_ref().is_some_and(|k| *k > 0.0))
            .count();
        let cost_pct = if opencode_n == 0 {
            None
        } else {
            share(opencode_with, opencode_n)
        };
        let pass = match (model_pct, cost_pct) {
            (Some(m), Some(k)) => Some(m >= MODEL_BAR && k >= COST_BAR),
            // no opencode runs in the window: cost share unmeasurable, model decides
            (Some(m), None) => Some(m >= MODEL_BAR),
            _ => None,
        };
        (model_pct, cost_pct, pass)
    };
    Models {
        total,
        with_client_model,
        with_cost,
        unknown_model_pct: pct(total - with_client_model, total),
        unknown_cost_pct: pct(total - with_cost, total),
        by_model,
        window: CostWindow {
            n,
            need: NEED,
            model_pct,
            cost_pct,
            pass,
        },
    }
}

/// The `handoff` a chunk shows: a dropped or parked chunk's reason is the
/// owner's close text, not the agent's older word — so `handoff` never reads
/// null on exactly the chunks that have a reason. Anything else keeps the
/// agent's last word.
fn handoff_for(
    handoffs: &mut HashMap<String, Handoff>,
    closes: &mut HashMap<String, Handoff>,
    c: &Chunk,
) -> Option<Handoff> {
    if matches!(c.state.as_str(), "dropped" | "parked")
        && let Some(close) = closes.remove(&c.uid)
    {
        return Some(close);
    }
    handoffs.remove(&c.uid)
}

#[cfg(test)]
mod models_tests {
    use super::*;

    fn row(
        client: Option<&str>,
        model: Option<&str>,
        cost: Option<f64>,
        started: &str,
    ) -> (
        Option<String>,
        Option<String>,
        Option<f64>,
        String,
        Option<String>,
    ) {
        (
            client.map(String::from),
            model.map(String::from),
            cost,
            started.into(),
            Some("/wt/1".into()),
        )
    }

    #[test]
    fn mixed_null_and_complete_renders_unknown_pct_exactly() {
        let rows = vec![
            row(
                Some("claude"),
                Some("opus"),
                Some(2.17),
                "2026-10-10T08:10:00Z",
            ),
            row(Some("claude"), Some("haiku"), None, "2026-10-10T07:00:00Z"),
            row(None, None, None, "2026-10-10T06:00:00Z"),
            row(
                None,
                Some("opencode-go/m"),
                Some(0.062652),
                "2026-10-24T07:47:50Z",
            ),
        ];
        let m = summarize(&rows);
        assert_eq!(m.total, 4);
        assert_eq!(m.with_client_model, 2);
        assert_eq!(m.with_cost, 2);
        assert_eq!(m.unknown_model_pct, 50.0);
        assert_eq!(m.unknown_cost_pct, 50.0);
        let opus = &m.by_model["claude/opus"];
        assert_eq!((opus.runs, opus.unknown_cost_pct), (1, 0.0));
        assert!((opus.cost - 2.17).abs() < 1e-9);
        let bare = &m.by_model["unknown/unknown"];
        assert_eq!((bare.runs, bare.unknown_cost_pct), (1, 100.0));
        // old rows predate the ship: window pending, never failing
        assert_eq!(m.window.n, 0);
        assert_eq!(
            (m.window.model_pct, m.window.cost_pct, m.window.pass),
            (None, None, None)
        );
    }

    #[test]
    fn empty_store_reads_zero_not_nan() {
        let m = summarize(&[]);
        assert_eq!(
            (m.total, m.unknown_model_pct, m.unknown_cost_pct),
            (0, 0.0, 0.0)
        );
        assert!(m.by_model.is_empty());
    }

    #[test]
    fn full_window_applies_the_80_50_bar() {
        let mut rows = Vec::new();
        for i in 0..30 {
            let complete = i < 24;
            let opencode = i >= 24;
            rows.push((
                Some(if opencode { "opencode" } else { "claude" }.into()),
                Some(if opencode { "opencode-go/m" } else { "opus" }.into()),
                if complete || (opencode && i < 27) {
                    Some(0.01)
                } else {
                    None
                },
                format!("2026-10-25T00:{i:02}:00Z"),
                Some("/wt/1".into()),
            ));
        }
        let m = summarize(&rows);
        assert_eq!(m.window.n, 30);
        // 30/30 model, 3/6 opencode cost = 50%: passes
        assert_eq!(m.window.model_pct, Some(100.0));
        assert_eq!(m.window.cost_pct, Some(50.0));
        assert_eq!(m.window.pass, Some(true));
    }

    #[test]
    fn zero_cost_reads_as_unknown_for_the_metric() {
        let rows = vec![
            row(
                Some("opencode"),
                Some("opencode-go/m"),
                Some(0.0),
                "2026-10-25T00:00:00Z",
            ),
            row(
                Some("opencode"),
                Some("opencode-go/m"),
                Some(0.01),
                "2026-10-25T00:01:00Z",
            ),
        ];
        let m = summarize(&rows);
        assert_eq!((m.with_cost, m.unknown_cost_pct), (1, 50.0));
        assert_eq!(m.by_model["opencode/opencode-go/m"].unknown_cost_pct, 50.0);
    }

    #[test]
    fn merge_sums_totals_and_recomputes_unknown_exactly() {
        let a = summarize(&[
            row(
                Some("claude"),
                Some("opus"),
                Some(2.0),
                "2026-10-10T08:00:00Z",
            ),
            row(Some("claude"), Some("opus"), None, "2026-10-10T09:00:00Z"),
        ]);
        let b = summarize(&[row(None, None, None, "2026-10-10T10:00:00Z")]);
        let m = Models::merge(&[a, b]);
        assert_eq!((m.total, m.with_client_model, m.with_cost), (3, 2, 1));
        assert_eq!((m.unknown_model_pct, m.unknown_cost_pct), (33.3, 66.7));
        let opus = &m.by_model["claude/opus"];
        assert_eq!(opus.runs, 2);
        assert!((opus.cost - 2.0).abs() < 1e-9);
        assert_eq!(opus.unknown_cost_pct, 50.0);
    }
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
