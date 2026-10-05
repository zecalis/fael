//! `fael tune` (SPEC-fael-learn-loop §C): replay each push policy candidate
//! over the search pushes already in `usage.jsonl` and set it beside
//! `baseline@1`. Read-only and pure — kept usage rows and loaded logs in, a
//! `Tune` out. It names no best candidate and picks no threshold: a candidate
//! is a rule that only cuts, judged on the rows it would have cut, and every
//! rate is `x/n` with its interval. Observations, not causes — the causal test
//! is the holdout (SPEC §E).

mod assoc;
mod cover;
mod rate;
mod replay;

use super::day::day_number;
use super::outcomes::{OUTCOMES_V, Observation, observations};
use super::parse::Parsed;
use crate::query::{BASELINE, TOUCH, TOUCH_YIELD};
use crate::{Log, Row, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

pub use assoc::{Assoc, Group};
pub use cover::{Coverage, MAX_DAY_SHARE_PCT, MAX_SESSION_SHARE_PCT, MIN_DAYS};
pub use rate::{Rate, wilson};
pub use replay::Used;

/// How many earlier sessions per history key count (`None` = all of them).
pub const DECAYS: [Option<usize>; 5] = [None, Some(20), Some(50), Some(100), Some(200)];
/// A stratum speaks for itself with this many sessions and search pushes.
pub const STRATUM_MIN_SESSIONS: usize = 10;
pub const STRATUM_MIN_PUSHES: usize = 100;

static NULL: serde_json::Value = serde_json::Value::Null;

/// One said search row with what a policy could have known when it was said.
pub(super) struct Ob<'a> {
    pub o: &'a Observation<'a>,
    pub row: Option<&'a Row>,
    pub feat: &'a serde_json::Value,
    /// Files of the row already in the session's working set; `None` = the
    /// line predates chunk 3 or had no session: unknown, not 0.
    pub touch: Option<usize>,
    /// Cited, pulled by the agent itself, or acted on.
    pub engaged: bool,
    pub file: &'a str,
    pub trigger: &'a str,
    pub client: &'a str,
    pub class: Option<String>,
    pub line: usize,
    /// Rows the line said, the denominator of a push left silent.
    pub said_n: usize,
}

impl<'a> Ob<'a> {
    fn new(o: &'a Observation<'a>, rows: &HashMap<(&str, &str), &'a Row>) -> Option<Ob<'a>> {
        let line = o.said_line.filter(|l| l["event"] == "search")?;
        let feat = &line["feat"][o.id];
        let feat = if feat.is_object() { feat } else { &NULL };
        let row = rows.get(&(o.repo, o.id)).copied();
        let files: Vec<&str> = line["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.as_str())
            .collect();
        let file = files
            .iter()
            .find(|f| row.is_some_and(|r| r.files.iter().any(|rf| rf == *f)))
            .or(files.first())
            .copied()
            .unwrap_or("");
        let class = feat["kind"]
            .as_str()
            .map(|k| format!("{k}/{}", feat["hub"].as_bool().unwrap_or(false)));
        Some(Ob {
            o,
            row,
            feat,
            touch: feat["touch"].as_u64().map(|t| t as usize),
            engaged: o.cited || o.agent_pull || o.acted,
            file,
            trigger: line["trigger"].as_str().unwrap_or(""),
            client: line["client"].as_str().unwrap_or("?"),
            class,
            line: std::ptr::from_ref(line) as usize,
            said_n: line["ids"].as_array().map_or(0, Vec::len),
        })
    }

    /// A policy can be replayed on it: its working-set count was recorded and
    /// the row is still in the log to call the rule on.
    fn evaluable(&self) -> bool {
        self.touch.is_some() && self.row.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Sizes {
    pub sessions: usize,
    pub search_pushes: usize,
    /// Rows said at a search push, and of those the ones a policy can be
    /// replayed on (working-set count recorded, row still in the log).
    pub rows_said: usize,
    pub evaluable_rows: usize,
    /// Rows the shadow itself listed in `would_drop` — what `touch@1`'s replay
    /// must reproduce.
    pub recorded_would_drop: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Retained {
    pub cited: Rate,
    pub pulled: Rate,
    pub acted: Rate,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Retrieved {
    /// Dropped rows the agent then pulled itself — diagnostic, per row.
    pub rows: Rate,
    /// Sessions that pulled a dropped row itself — the safety view, per session.
    pub sessions: Rate,
}

/// One candidate against the rows it can be replayed on.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PolicyResult {
    pub policy: String,
    pub dropped: usize,
    /// Rows still said / rows said — per push.
    pub exposure: Rate,
    /// Pushes left with nothing to say / pushes.
    pub pushes_silenced: Rate,
    /// Outcomes on rows the policy keeps / outcomes on all of them.
    pub retained: Retained,
    pub retrieved_after_cut: Retrieved,
    /// Dropped rows the agent pulled itself and then cited or acted on.
    pub missed_push: Rate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecayPoint {
    pub decay: Option<usize>,
    pub result: PolicyResult,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Section {
    pub sizes: Sizes,
    /// `baseline@1`, `touch@1`, `touch-yield@1` (decay: none).
    pub policies: Vec<PolicyResult>,
    /// `touch-yield@1` at each decay, and which history level spoke (decay: none).
    pub decay_sweep: Vec<DecayPoint>,
    pub fallback_used: Used,
    pub association: Vec<Assoc>,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stratum {
    pub repo: String,
    pub client: String,
    /// Enough sessions and search pushes to be read on its own — not a quality
    /// gate: an ineligible stratum is reported and never fails a policy.
    pub eligible: bool,
    pub section: Section,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tune {
    pub outcomes_v: u32,
    /// First and last local day with a search push.
    pub days: Option<(String, String)>,
    pub all: Section,
    pub strata: Vec<Stratum>,
}

fn result(policy: String, obs: &[&Ob], drops: &[bool]) -> PolicyResult {
    let mut rows = 0;
    let (mut dropped, mut retrieved, mut missed) = (0, 0, 0);
    let (mut all, mut kept) = ([0; 3], [0; 3]);
    let mut lines: HashMap<usize, (usize, usize)> = HashMap::new();
    let (mut sessions, mut pulled_in) = (HashSet::new(), HashSet::new());
    for (o, &d) in obs.iter().zip(drops).filter(|(o, _)| o.evaluable()) {
        rows += 1;
        sessions.insert((o.o.repo, o.o.session));
        let line = lines.entry(o.line).or_insert((o.said_n, 0));
        line.1 += d as usize;
        let flags = [o.o.cited, o.o.agent_pull, o.o.acted];
        for (k, hit) in flags.into_iter().enumerate().filter(|(_, h)| *h) {
            all[k] += hit as usize;
            kept[k] += !d as usize;
        }
        if d {
            dropped += 1;
            if o.o.agent_pull {
                retrieved += 1;
                pulled_in.insert((o.o.repo, o.o.session));
            }
            missed += o.o.pulled_then_used as usize;
        }
    }
    PolicyResult {
        policy,
        dropped,
        exposure: Rate::new(rows - dropped, rows),
        pushes_silenced: Rate::new(
            lines.values().filter(|(said, d)| d >= said).count(),
            lines.len(),
        ),
        retained: Retained {
            cited: Rate::new(kept[0], all[0]),
            pulled: Rate::new(kept[1], all[1]),
            acted: Rate::new(kept[2], all[2]),
        },
        retrieved_after_cut: Retrieved {
            rows: Rate::new(retrieved, dropped),
            sessions: Rate::new(pulled_in.len(), sessions.len()),
        },
        missed_push: Rate::new(missed, dropped),
    }
}

/// One stratum's search pushes and said rows.
type Stratified<'a> = (Vec<&'a cover::Push<'a>>, Vec<&'a Ob<'a>>);

fn section(pushes: &[&cover::Push], obs: &[&Ob]) -> Section {
    let none = vec![false; obs.len()];
    let (touch, (yield_none, used)) = (replay::touch(obs), replay::touch_yield(obs, None));
    let sweep = DECAYS.map(|decay| {
        let drops = replay::touch_yield(obs, decay).0;
        DecayPoint {
            decay,
            result: result(TOUCH_YIELD.name(), obs, &drops),
        }
    });
    let eval: Vec<&&Ob> = obs.iter().filter(|o| o.evaluable()).collect();
    let coverage = cover::coverage(pushes);
    Section {
        sizes: Sizes {
            sessions: coverage.sessions,
            search_pushes: pushes.len(),
            rows_said: obs.len(),
            evaluable_rows: eval.len(),
            recorded_would_drop: obs
                .iter()
                .filter(|o| o.o.cut.contains("would_drop"))
                .count(),
        },
        policies: vec![
            result(BASELINE.name(), obs, &none),
            result(TOUCH.name(), obs, &touch),
            result(TOUCH_YIELD.name(), obs, &yield_none),
        ],
        decay_sweep: sweep.into(),
        fallback_used: used,
        association: assoc::assoc(&eval.iter().map(|o| **o).collect::<Vec<_>>()),
        coverage,
    }
}

/// Every search push and the rows it said, replayed. `tz_min` picks the local
/// day coverage counts.
pub fn tune(parsed: &Parsed, logs: &HashMap<String, Log>, tz_min: i32) -> Tune {
    let rows: HashMap<(&str, &str), &Row> = logs
        .iter()
        .flat_map(|(repo, l)| {
            l.rows
                .iter()
                .map(move |r| ((repo.as_str(), r.id.as_str()), r))
        })
        .collect();
    let seen = observations(parsed, logs);
    let obs: Vec<Ob> = seen.iter().filter_map(|o| Ob::new(o, &rows)).collect();
    let pushes: Vec<cover::Push> = parsed
        .kept
        .iter()
        .filter(|v| v["event"] == "search")
        .filter_map(|v| {
            Some(cover::Push {
                repo: v["repo"].as_str()?,
                client: v["client"].as_str().unwrap_or("?"),
                session: v["session"].as_str()?,
                day: day_number(v["ts"].as_str().and_then(ts_ms)?, tz_min),
            })
        })
        .collect();
    let days = pushes.iter().map(|p| p.day);
    let span = days.clone().min().zip(days.max());
    let (all_p, all_o): (Vec<&cover::Push>, Vec<&Ob>) =
        (pushes.iter().collect(), obs.iter().collect());
    let mut by: BTreeMap<(&str, &str), Stratified> = BTreeMap::new();
    for p in &pushes {
        by.entry((p.repo, p.client)).or_default().0.push(p);
    }
    for o in &obs {
        by.entry((o.o.repo, o.client)).or_default().1.push(o);
    }
    let mut strata: Vec<Stratum> = by
        .into_iter()
        .map(|((repo, client), (p, o))| {
            let section = section(&p, &o);
            Stratum {
                repo: repo.to_string(),
                client: client.to_string(),
                eligible: section.sizes.sessions >= STRATUM_MIN_SESSIONS
                    && section.sizes.search_pushes >= STRATUM_MIN_PUSHES,
                section,
            }
        })
        .collect();
    strata.sort_by_key(|s| std::cmp::Reverse(s.section.sizes.search_pushes));
    Tune {
        outcomes_v: OUTCOMES_V,
        days: span.map(|(a, b)| (cover::civil(a), cover::civil(b))),
        all: section(&all_p, &all_o),
        strata,
    }
}

#[cfg(test)]
mod tests;
