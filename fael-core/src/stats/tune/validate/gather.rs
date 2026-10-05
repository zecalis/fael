//! Raw counts of the two arms, per stratum, from the usage lines.

use super::super::cover::{self, Push};
use super::super::rate::Rate;
use super::ArmSize;
use crate::query::{ARM_CANDIDATE, ARM_HOLDOUT};
use crate::stats::day::day_number;
use crate::stats::outcomes::Observation;
use crate::stats::parse::Parsed;
use crate::ts_ms;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// One arm's raw counts before they become an `ArmSize`.
#[derive(Default)]
pub(super) struct Acc<'a> {
    pub(super) pushes: Vec<Push<'a>>,
    pub(super) rows_said: usize,
    pub(super) triggers: BTreeMap<&'a str, usize>,
    pub(super) gate_cuts: usize,
    pub(super) missed: usize,
    pub(super) retrieved: HashSet<(&'a str, &'a str)>,
    pub(super) retrieved_rows: usize,
    pub(super) dup: HashSet<(&'a str, &'a str)>,
}

impl<'a> Acc<'a> {
    pub(super) fn size(&self) -> ArmSize {
        let coverage = cover::coverage(&self.pushes.iter().collect::<Vec<_>>());
        ArmSize {
            sessions: coverage.sessions,
            search_pushes: self.pushes.len(),
            rows_said: self.rows_said,
            gate_cuts: self.gate_cuts,
            missed_push: Rate::new(self.missed, self.gate_cuts),
            retrieved_sessions: Rate::new(self.retrieved.len(), coverage.sessions),
            retrieved_rows: self.retrieved_rows,
            dup_sessions: Rate::new(self.dup.len(), coverage.sessions),
            triggers: self
                .triggers
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            coverage,
        }
    }

    pub(super) fn merge(&mut self, o: &Acc<'a>) {
        self.pushes.extend(&o.pushes);
        self.rows_said += o.rows_said;
        self.gate_cuts += o.gate_cuts;
        self.missed += o.missed;
        self.retrieved.extend(&o.retrieved);
        self.retrieved_rows += o.retrieved_rows;
        self.dup.extend(&o.dup);
        for (k, v) in &o.triggers {
            *self.triggers.entry(k).or_default() += v;
        }
    }
}

/// [candidate, holdout]
pub(super) type Pair<'a> = [Acc<'a>; 2];

/// One repo scope: its client strata and the candidate policies its pushes named.
#[derive(Default)]
pub(super) struct RepoArms<'a> {
    pub(super) clients: BTreeMap<&'a str, Pair<'a>>,
    pub(super) policies: BTreeSet<&'a str>,
}

/// By repo scope. Nothing here is ever pooled across two scopes.
pub(super) type Gathered<'a> = BTreeMap<String, RepoArms<'a>>;

/// Every search push and observation of an arm, by repo scope and client
/// stratum. `scope` maps the usage line's `repo` (a worktree root) to the
/// repo it belongs to. `None` = no push carries an arm.
pub(super) fn gather<'a>(
    parsed: &'a Parsed,
    seen: &[Observation<'a>],
    tz_min: i32,
    scope: &dyn Fn(&str) -> String,
) -> Option<Gathered<'a>> {
    let mut by = Gathered::new();
    let mut memo: HashMap<&str, String> = HashMap::new();
    let mut who: HashMap<(&str, &str), (&str, usize)> = HashMap::new();
    for v in parsed.kept.iter().filter(|v| v["event"] == "search") {
        let arm = match v["arm"].as_str() {
            Some(ARM_CANDIDATE) => 0,
            Some(ARM_HOLDOUT) => 1,
            _ => continue,
        };
        let (Some(repo), Some(session), Some(ms)) = (
            v["repo"].as_str(),
            v["session"].as_str(),
            v["ts"].as_str().and_then(ts_ms),
        ) else {
            continue;
        };
        let client = v["client"].as_str().unwrap_or("?");
        let r = by
            .entry(memo.entry(repo).or_insert_with(|| scope(repo)).clone())
            .or_default();
        if arm == 0 {
            r.policies.extend(v["policy"].as_str());
        }
        who.insert((repo, session), (client, arm));
        let a = &mut r.clients.entry(client).or_default()[arm];
        a.pushes.push(Push {
            repo,
            client,
            session,
            day: day_number(ms, tz_min),
        });
        a.rows_said += v["ids"].as_array().map_or(0, Vec::len);
        *a.triggers
            .entry(v["trigger"].as_str().unwrap_or(""))
            .or_default() += 1;
    }
    if by.is_empty() {
        return None;
    }
    // `dup` rides an outcome line of the session that filed the row
    let dups = parsed
        .kept
        .iter()
        .filter(|v| v["event"] == "outcome" && v["dup"].is_array());
    for v in dups {
        let (Some(repo), Some(session)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        let (Some(&(client, arm)), Some(r)) = (
            who.get(&(repo, session)),
            memo.get(repo).and_then(|s| by.get_mut(s)),
        ) else {
            continue;
        };
        if let Some(c) = r.clients.get_mut(client) {
            c[arm].dup.insert((repo, session));
        }
    }
    for o in seen {
        let Some(&(client, arm)) = who.get(&(o.repo, o.session)) else {
            continue;
        };
        let Some(r) = memo.get(o.repo).and_then(|s| by.get_mut(s)) else {
            continue;
        };
        let a = &mut r.clients.get_mut(client)?[arm];
        if o.cut.contains(crate::query::CUT_GATE) {
            a.gate_cuts += 1;
            a.missed += o.missed_push as usize;
        }
        // the shadow cut is the holdout's own bookkeeping, not a cut
        if o.retrieved && o.cut.iter().any(|c| c != "would_drop") {
            a.retrieved.insert((o.repo, o.session));
            a.retrieved_rows += 1;
        }
    }
    Some(by)
}
