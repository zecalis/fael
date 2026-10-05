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
struct Obs {
    said: bool,
    cut: BTreeSet<String>,
    cut_ms: Option<i64>,
    cited: bool,
    first_said_ms: i64,
    agent_pull: bool,
    induced_pull: bool,
    /// First agent-initiated pull of the row while it was cut and unsaid.
    retrieved_ms: Option<i64>,
    cited_after_retrieval: bool,
}

#[derive(Default)]
struct Session<'a> {
    obs: HashMap<&'a str, Obs>,
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
                (o.said, o.first_said_ms) = (true, ms);
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
                *(if induced {
                    &mut o.induced_pull
                } else {
                    &mut o.agent_pull
                }) = true;
            } else if o.cut_ms.is_some() && !induced {
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
    }
}

/// Outcomes per row id over every session in `parsed`.
pub(super) fn outcomes(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
) -> BTreeMap<String, RowOutcomes> {
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
    let mut out: BTreeMap<String, RowOutcomes> = BTreeMap::new();
    for ((repo, _), st) in &sessions {
        // closed, superseded or bumped in the day from `ms`
        let acted_from = |id: &str, ms: i64| {
            gone.get(repo)
                .and_then(|t| t.get(id))
                .is_some_and(|g| *g >= ms && *g - ms <= RETIRE_WINDOW_MS)
        };
        for (id, o) in &st.obs {
            if !o.said && o.cut.is_empty() {
                continue; // only pulled or cited: nothing fael decided
            }
            let r = out.entry(id.to_string()).or_default();
            r.shown += o.said as usize;
            for why in &o.cut {
                *r.cut.entry(why.clone()).or_default() += 1;
            }
            r.cited += o.cited as usize;
            r.pulled.agent_initiated += o.agent_pull as usize;
            r.pulled.fael_induced += o.induced_pull as usize;
            r.acted += (o.said && acted_from(id, o.first_said_ms)) as usize;
            if let Some(ms) = o.retrieved_ms {
                r.retrieved_after_cut += 1;
                let policy = POLICY_CUTS.iter().any(|p| o.cut.contains(*p));
                r.missed_push +=
                    (policy && (o.cited_after_retrieval || acted_from(id, ms))) as usize;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// A push-shaped usage line of session `s1` at minute `min`.
    fn line(min: u8, rest: &str) -> String {
        format!(
            "{{\"ts\":\"2026-10-05T00:{min:02}:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"s1\",{rest}}}\n"
        )
    }

    fn run(usage: &str, closes: &str) -> BTreeMap<String, RowOutcomes> {
        let row = |id: &str| {
            format!(
                "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-04T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[\"a.rs\"]}}\n"
            )
        };
        let mut log = Log {
            rows: super::super::said::tests::rows(&["A", "G", "H", "I", "N"].map(row).concat()),
            ..Log::default()
        };
        log.closes = super::super::said::tests::rows(closes);
        let p = super::super::parse::parse(
            usage,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        outcomes(&p, &HashMap::from([("/w/r".to_string(), log)]))
    }

    fn close(id: &str, ts: &str) -> String {
        format!(
            "{{\"v\":1,\"id\":\"C{id}\",\"ts\":\"{ts}\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"{id}\"}}\n"
        )
    }

    #[test]
    fn each_outcome_is_observed_and_its_fail_examples_are_not() {
        let usage = line(
            0,
            r#""event":"search","ids":["A","N"],"said":[{"kind":"row","key":"A"},{"kind":"pointer","key":"k:x"},{"kind":"count","key":"a.rs|file"}],"cut":[{"id":"G","r":"gate"},{"id":"H","r":"gate"},{"id":"I","r":"cap"}]"#,
        ) + &line(1, r#""event":"outcome","ids":[],"cited":["A","Z"]"#)
            // a pointer key, then a by-id pull of a shown row
            + &line(2, r#""event":"find","found":["A"],"q":{"key":"k:x"}"#)
            + &line(3, r#""event":"find","found":["N"],"q":{"id":"N"}"#)
            // G pulled by the agent, H through the count line's call, I by the agent
            + &line(4, r#""event":"find","found":["G"],"q":{"id":"G"}"#)
            + &line(5, r#""event":"find","found":["H"],"q":{"files":["a.rs"]}"#)
            + &line(6, r#""event":"find","found":["I"],"q":{"id":"I"}"#)
            + &line(7, r#""event":"outcome","ids":[],"cited":["G","I"]"#);
        let o = run(&usage, &close("A", "2026-10-05T01:00:00Z"));
        let a = &o["A"];
        assert_eq!((a.shown, a.cited, a.acted), (1, 1, 1), "{a:?}");
        assert_eq!(
            a.pulled,
            Pulled {
                agent_initiated: 0,
                fael_induced: 1
            }
        );
        assert!(!o.contains_key("Z"), "an id fael never said is no cite");
        assert_eq!(
            o["N"].pulled.agent_initiated, 1,
            "a by-id pull of a shown row"
        );
        // gate cut, agent pulled it, then cited it
        assert_eq!((o["G"].retrieved_after_cut, o["G"].missed_push), (1, 1));
        assert_eq!(o["G"].cut["gate"], 1);
        // gate cut, pulled only through the count line fael said
        assert_eq!((o["H"].retrieved_after_cut, o["H"].missed_push), (0, 0));
        // cap cut: the agent came back and cited it, but no policy cut it
        assert_eq!((o["I"].retrieved_after_cut, o["I"].missed_push), (1, 0));
        assert_eq!(o["I"].cut["cap"], 1);
    }

    #[test]
    fn a_pull_without_a_cite_or_act_is_no_missed_push() {
        let usage = line(0, r#""event":"search","ids":[],"cut":[{"id":"G","r":"gate"}]"#)
            + &line(1, r#""event":"find","found":["G"],"q":{"id":"G"}"#)
            + &line(2, r#""event":"outcome","ids":[],"cited":["G"]"#)
            // a second row: pulled, then closed within the day
            + &line(3, r#""event":"search","ids":[],"would_drop":{"policy":"touch@1","ids":["H"]},"cut":[]"#)
            + &line(4, r#""event":"find","found":["H"],"q":{"id":"H"}"#);
        let o = run(&usage, &close("H", "2026-10-05T02:00:00Z"));
        assert_eq!((o["G"].retrieved_after_cut, o["G"].missed_push), (1, 1));
        assert_eq!(o["H"].cut["would_drop"], 1);
        assert_eq!((o["H"].retrieved_after_cut, o["H"].missed_push), (1, 1));
        let usage = line(
            0,
            r#""event":"search","ids":[],"cut":[{"id":"G","r":"gate"}]"#,
        ) + &line(1, r#""event":"find","found":["G"],"q":{"id":"G"}"#);
        let o = run(&usage, "");
        assert_eq!((o["G"].retrieved_after_cut, o["G"].missed_push), (1, 0));
    }
}
