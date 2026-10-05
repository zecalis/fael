//! Observed outcomes per row (SPEC-fael-learn-loop §B, `outcomes_v: 1`): what
//! happened to a row fael said or cut. Observations, never weights and never a
//! causal claim. Unit = one (repo, session, row): a session that was told a
//! row, or had it cut, counts once however many pushes repeated it. Pure:
//! kept usage rows and loaded logs in.
//!
//! A pull has a provenance. `fael_induced` = the query is one a line fael said
//! earlier in the session printed (a pointer's key, a count's call, an edit
//! hint's id); `agent_initiated` = anything else. Only an agent-initiated pull
//! is evidence for `retrieved_after_cut` and `missed_push` — an induced one
//! would be fael measuring its own suggestion.

use super::parse::Parsed;
use super::retire::{RETIRE_WINDOW_MS, retire_times};
use super::said::{Pull, counted, strs};
use crate::{Log, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Bump when an outcome's definition changes; `tune` reports per version.
pub const OUTCOMES_V: u32 = 1;

/// Cut reasons that are a policy's decision, not a system limit (SPEC §A).
const POLICY_CUTS: [&str; 2] = ["gate", "would_drop"];

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Pulled {
    pub agent_initiated: usize,
    pub fael_induced: usize,
}

/// One row's outcomes, each a count of sessions.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RowOutcomes {
    /// Sessions fael said the row in.
    pub shown: usize,
    /// Sessions the row was cut in, per reason (`cap`, `hub_peek`, `budget`,
    /// `gate`, `would_drop`) — a row cut for two reasons counts under both.
    pub cut: BTreeMap<String, usize>,
    /// Said, then its id typed in a later tool input or reply.
    pub cited: usize,
    /// Said, then a later pull showed its body.
    pub pulled: Pulled,
    /// Said, then closed, superseded or bumped within a day.
    pub acted: usize,
    /// Cut, then the agent pulled it itself.
    pub retrieved_after_cut: usize,
    /// A policy-cut row the agent pulled itself and then cited or acted on.
    pub missed_push: usize,
}

#[derive(Default)]
struct Obs<'a> {
    said: bool,
    /// The push line that said it — `tune` reads its trigger and features.
    said_line: Option<&'a serde_json::Value>,
    cut: BTreeSet<String>,
    cut_ms: Option<i64>,
    cited: bool,
    first_said_ms: i64,
    agent_pull: bool,
    /// When the agent first pulled the row after fael said it.
    agent_pull_ms: Option<i64>,
    cited_after_pull: bool,
    induced_pull: bool,
    /// First agent-initiated pull of the row while it was cut and unsaid.
    retrieved_ms: Option<i64>,
    cited_after_retrieval: bool,
}

#[derive(Default)]
struct Session<'a> {
    obs: HashMap<&'a str, Obs<'a>>,
    /// The session's last usage line: what a later session may learn from.
    end_ms: i64,
    pointers: HashSet<&'a str>,
    counts: Vec<&'a str>,
    asks: HashSet<&'a str>,
}

impl<'a> Session<'a> {
    /// A push or brief line: the rows it said and cut, and the lines whose
    /// calls a later pull may be taking.
    fn line(&mut self, v: &'a serde_json::Value, ms: i64) {
        for id in strs(v, "ids") {
            let o = self.obs.entry(id).or_default();
            if !o.said {
                (o.said, o.first_said_ms, o.said_line) = (true, ms, Some(v));
            }
        }
        let cuts = v["cut"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| Some((c["id"].as_str()?, c["r"].as_str()?)));
        let dropped = strs(&v["would_drop"], "ids").map(|id| (id, "would_drop"));
        for (id, why) in cuts.chain(dropped) {
            let o = self.obs.entry(id).or_default();
            o.cut.insert(why.to_string());
            o.cut_ms.get_or_insert(ms);
        }
        for e in v["said"].as_array().into_iter().flatten() {
            let Some(key) = e["key"].as_str() else {
                continue;
            };
            match e["kind"].as_str() {
                Some("pointer") => {
                    self.pointers.insert(key);
                }
                Some("ask") => {
                    self.asks.insert(key);
                }
                Some("count") => self.counts.push(key),
                _ => {}
            }
        }
    }

    fn induced(&self, p: &Pull) -> bool {
        p.key.is_some_and(|k| self.pointers.contains(k))
            || p.id.is_some_and(|i| self.asks.contains(i))
            || self.counts.iter().any(|c| counted(c, p))
    }

    fn pull(&mut self, p: &Pull, found: &[&'a str]) {
        let induced = self.induced(p);
        for id in found {
            let o = self.obs.entry(id).or_default();
            if o.said {
                if induced {
                    o.induced_pull = true;
                } else {
                    o.agent_pull = true;
                    o.agent_pull_ms.get_or_insert(p.ms);
                }
            }
            // a `would_drop` row was said (shadow: the agent saw it), so its pull
            // is the demand a `gate` cut would have drawn; every other cut row
            // counts only while it is still unsaid
            if !induced && o.cut_ms.is_some() && (!o.said || o.cut.contains("would_drop")) {
                o.retrieved_ms.get_or_insert(p.ms);
            }
        }
    }

    /// A `cited` outcome line: the id was typed after the line that said it.
    /// A pulled cut row is no said row, but its cite is what `missed_push` reads.
    fn cite(&mut self, id: &'a str) {
        let o = self.obs.entry(id).or_default();
        o.cited |= o.said;
        o.cited_after_retrieval |= o.retrieved_ms.is_some();
        o.cited_after_pull |= o.agent_pull_ms.is_some();
    }
}

/// One row in one session — the unit every outcome counts, with the push line
/// that said it, so `tune` can replay a policy over the same observations.
pub(super) struct Observation<'a> {
    pub repo: &'a str,
    pub session: &'a str,
    pub id: &'a str,
    pub said_line: Option<&'a serde_json::Value>,
    pub first_said_ms: i64,
    /// The session's last usage line: its outcomes are known after this.
    pub end_ms: i64,
    pub cut: BTreeSet<String>,
    pub cited: bool,
    pub agent_pull: bool,
    pub induced_pull: bool,
    pub acted: bool,
    pub retrieved: bool,
    pub missed_push: bool,
    /// The agent pulled the row itself after it was said, then cited or acted
    /// on it — what a policy that had dropped the said row would have cost.
    pub pulled_then_used: bool,
}

/// Every (repo, session, row) fael said or cut, with what followed.
pub(super) fn observations<'a>(
    parsed: &'a Parsed,
    logs: &HashMap<String, Log>,
) -> Vec<Observation<'a>> {
    let mut sessions: HashMap<(&str, &str), Session> = HashMap::new();
    for v in &parsed.kept {
        let (Some(repo), Some(s), Some(ms)) = (
            v["repo"].as_str(),
            v["session"].as_str(),
            v["ts"].as_str().and_then(ts_ms),
        ) else {
            continue;
        };
        let st = sessions.entry((repo, s)).or_default();
        st.end_ms = st.end_ms.max(ms);
        if v["event"] == "in-context" {
            continue;
        } else if v.get("found").is_some() {
            let q = &v["q"];
            let pull = Pull {
                ms,
                key: q["key"].as_str(),
                files: strs(q, "files").collect(),
                id: q["id"].as_str(),
            };
            st.pull(&pull, &strs(v, "found").collect::<Vec<_>>());
        } else if v["event"] == "outcome" {
            strs(v, "cited").for_each(|id| st.cite(id));
        } else {
            st.line(v, ms);
        }
    }
    let gone: HashMap<&str, HashMap<&str, i64>> = logs
        .iter()
        .map(|(repo, log)| (repo.as_str(), retire_times(log)))
        .collect();
    let mut out = Vec::new();
    for (&(repo, session), st) in &sessions {
        // closed, superseded or bumped in the day from `ms`
        let acted_from = |id: &str, ms: i64| {
            gone.get(repo)
                .and_then(|t| t.get(id))
                .is_some_and(|g| *g >= ms && *g - ms <= RETIRE_WINDOW_MS)
        };
        for (&id, o) in &st.obs {
            if !o.said && o.cut.is_empty() {
                continue; // only pulled or cited: nothing fael decided
            }
            let policy = POLICY_CUTS.iter().any(|p| o.cut.contains(*p));
            out.push(Observation {
                repo,
                session,
                id,
                said_line: o.said_line,
                first_said_ms: o.first_said_ms,
                end_ms: st.end_ms,
                cut: o.cut.clone(),
                cited: o.cited,
                agent_pull: o.agent_pull,
                induced_pull: o.induced_pull,
                acted: o.said && acted_from(id, o.first_said_ms),
                retrieved: o.retrieved_ms.is_some(),
                missed_push: o
                    .retrieved_ms
                    .is_some_and(|ms| policy && (o.cited_after_retrieval || acted_from(id, ms))),
                pulled_then_used: o
                    .agent_pull_ms
                    .is_some_and(|ms| o.cited_after_pull || acted_from(id, ms)),
            });
        }
    }
    out
}

/// Outcomes per row id over every session in `parsed`.
pub(super) fn outcomes(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
) -> BTreeMap<String, RowOutcomes> {
    let mut out: BTreeMap<String, RowOutcomes> = BTreeMap::new();
    for o in observations(parsed, logs) {
        let r = out.entry(o.id.to_string()).or_default();
        r.shown += o.said_line.is_some() as usize;
        for why in &o.cut {
            *r.cut.entry(why.clone()).or_default() += 1;
        }
        r.cited += o.cited as usize;
        r.pulled.agent_initiated += o.agent_pull as usize;
        r.pulled.fael_induced += o.induced_pull as usize;
        r.acted += o.acted as usize;
        r.retrieved_after_cut += o.retrieved as usize;
        r.missed_push += o.missed_push as usize;
    }
    out
}

#[cfg(test)]
mod tests;
