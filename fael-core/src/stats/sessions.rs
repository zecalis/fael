//! One row per session (issue 01M75KQ5): what a session was handed and what
//! it did, from the same parsed lines the aggregates read — so a stranger can
//! check a number against the session that made it. A description, never a
//! claim: the hit and acted rules are `value.rs`'s and `touch.rs`'s, called
//! here, not restated. MCP lines carry no session and stay out. Pure: parsed
//! usage, loaded logs and crew.db runs in.

use super::FlowRun;
use super::capture::{by_time, filed, spans};
use super::friction::{is_agent_call, is_gate_reject};
use super::parse::{Parsed, session_stem};
use super::value::{acted_pairs, pairs};
use crate::{Log, rfc3339, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// One session's numbers, keyed by (repo, session stem).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionView {
    pub session: String,
    pub repo: String,
    /// The client on the session's push lines (its call lines' when it had none).
    pub client: String,
    /// First and last usage line of the session, RFC 3339 UTC.
    pub first: String,
    pub last: String,
    /// Branch of the session's last edit; absent when it never edited.
    pub branch: Option<String>,
    pub edits: usize,
    /// Est tokens fael pushed into the session, by usage event.
    pub pushed_tokens: BTreeMap<String, usize>,
    /// Distinct rows a push handed over.
    pub shown: usize,
    /// …of those, the ones pushed at or after the client's first `in-context`
    /// line: the only pushes that could score, and the denominator the next two share.
    pub scored: usize,
    /// …of which still in context when the agent edited the row's file.
    pub in_context: usize,
    /// …of which the session acted on (in context, cited, found, or touched the file).
    pub acted: usize,
    /// Agent calls to fael (shape-gate rejects are no call, as in `friction`).
    pub calls: usize,
    pub rejects: usize,
    /// Rows the session filed; `None` when its repo's log was not loaded.
    pub rows_filed: Option<usize>,
    /// A crew run's session equals this one (the join `handoff.rs` uses).
    pub in_run: bool,
}

/// One group of the split tables, over sessions with at least one edit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Group {
    pub sessions: usize,
    pub edits: usize,
    pub calls: usize,
    pub rejects: usize,
    pub rows_filed: usize,
    /// Sessions whose log was loaded and that filed no row.
    pub no_row: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Split {
    pub with: Group,
    pub without: Group,
}

/// Two descriptive splits of edited sessions: rows pushed ≥ 1 against none,
/// inside a chunk run against outside. The groups differ in task, client and
/// repo; no number here is an effect of fael.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Splits {
    pub pushed: Split,
    pub in_run: Split,
}

#[derive(Default)]
struct Acc<'a> {
    client: &'a str,
    edits: usize,
    toks: BTreeMap<String, usize>,
    calls: usize,
    rejects: usize,
}

/// Pairs per (repo, session).
fn count<'a>(
    keys: impl Iterator<Item = &'a (&'a str, &'a str, &'a str)>,
) -> HashMap<(&'a str, &'a str), usize> {
    let mut m = HashMap::new();
    for &(r, s, _) in keys {
        *m.entry((r, s)).or_default() += 1;
    }
    m
}

pub fn sessions(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    runs: &[FlowRun],
) -> (Vec<SessionView>, Splits) {
    let spans = spans(parsed);
    let (all, _) = pairs(parsed, false);
    let (scored, in_ctx) = pairs(parsed, true);
    let acted = acted_pairs(parsed, logs, &scored, &in_ctx);
    let mut acc: HashMap<(&str, &str), Acc> = HashMap::new();
    for v in &parsed.kept {
        let (Some(repo), Some(s)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        let a = acc.entry((repo, s)).or_default();
        if a.client.is_empty() {
            a.client = v["client"].as_str().unwrap_or("");
        }
        a.edits += usize::from(v["event"] == "edit");
    }
    for v in parsed.calls.iter().filter(|v| is_agent_call(v)) {
        let (Some(repo), Some(s)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        let a = acc.entry((repo, s)).or_default();
        if a.client.is_empty() {
            a.client = v["client"].as_str().unwrap_or("");
        }
        if is_gate_reject(v)
            || v["ts"].as_str().and_then(ts_ms).is_none()
            || !matches!(v["outcome"].as_str(), Some("ok" | "reject" | "help"))
        {
            continue;
        }
        a.calls += 1;
        a.rejects += usize::from(v["outcome"] == "reject");
    }
    for r in parsed.rows.iter().filter(|r| !r.session.is_empty()) {
        *acc.entry((&r.repo, &r.session))
            .or_default()
            .toks
            .entry(r.event.clone())
            .or_default() += r.toks;
    }
    let (shown, scored_n) = (count(all.keys()), count(scored.keys()));
    let (hit, did) = (count(in_ctx.iter()), count(acted.iter()));
    let by_ms = by_time(logs);
    let in_runs: HashSet<&str> = runs
        .iter()
        .filter_map(|r| r.session.as_deref().map(session_stem))
        .collect();
    let n = |m: &HashMap<(&str, &str), usize>, k| m.get(&k).copied().unwrap_or(0);
    let at = |ms: i64| rfc3339(ms.max(0) as u64);
    let mut out: Vec<SessionView> = acc
        .into_iter()
        .filter_map(|(k, a)| {
            let span = spans.get(&k)?;
            Some(SessionView {
                session: k.1.to_string(),
                repo: k.0.to_string(),
                client: a.client.to_string(),
                first: at(span.0),
                last: at(span.1),
                branch: span.3.map(String::from),
                edits: a.edits,
                pushed_tokens: a.toks,
                shown: n(&shown, k),
                scored: n(&scored_n, k),
                in_context: n(&hit, k),
                acted: n(&did, k),
                calls: a.calls,
                rejects: a.rejects,
                rows_filed: by_ms.get(k.0).map(|rows| filed(rows, k.1, span).len()),
                in_run: in_runs.contains(k.1),
            })
        })
        .collect();
    out.sort_by(|a, b| (&b.last, &a.session, &a.repo).cmp(&(&a.last, &b.session, &b.repo)));
    let splits = splits(&out);
    (out, splits)
}

fn splits(list: &[SessionView]) -> Splits {
    let mut s = Splits::default();
    for v in list.iter().filter(|v| v.edits > 0) {
        for (split, with) in [(&mut s.pushed, v.shown > 0), (&mut s.in_run, v.in_run)] {
            let g = if with {
                &mut split.with
            } else {
                &mut split.without
            };
            g.sessions += 1;
            g.edits += v.edits;
            g.calls += v.calls;
            g.rejects += v.rejects;
            g.rows_filed += v.rows_filed.unwrap_or(0);
            g.no_row += usize::from(v.rows_filed == Some(0));
        }
    }
    s
}

/// The sessions `q` names: a full stem or a unique prefix of one. One stem in
/// several repos returns each; a prefix of several stems is an error naming them.
pub fn find_session<'a>(list: &'a [SessionView], q: &str) -> Result<Vec<&'a SessionView>, String> {
    let q = session_stem(q);
    let exact: Vec<_> = list.iter().filter(|v| v.session == q).collect();
    if !exact.is_empty() {
        return Ok(exact);
    }
    let hits: Vec<_> = list.iter().filter(|v| v.session.starts_with(q)).collect();
    let stems: HashSet<&str> = hits.iter().map(|v| v.session.as_str()).collect();
    match stems.len() {
        0 => Err(format!(
            "rejected: no session in the window starts with {q:?}"
        )),
        1 => Ok(hits),
        _ => {
            let mut s: Vec<_> = stems.into_iter().collect();
            s.sort_unstable();
            Err(format!(
                "rejected: {q:?} starts {} sessions — longer prefix: {}",
                s.len(),
                s.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::parse::parse;
    use std::path::Path;

    fn line(ts: &str, session: &str, rest: &str) -> String {
        format!(r#"{{"ts":"{ts}","repo":"/w/r","client":"claude","session":"{session}",{rest}}}"#)
    }

    /// s1: pushed row A, edited its file (a hit), one ok and one rejected
    /// call; s2: edited, was handed nothing.
    fn fixture() -> Parsed {
        let lines = [
            line(
                "2026-10-01T00:00:00.000Z",
                "s0",
                r#""event":"in-context","in_context":["Z"],"files":[]"#,
            ),
            line(
                "2026-10-02T00:00:00.000Z",
                "s1",
                r#""event":"session-start","bytes":9,"est_tokens":3,"ids":["A"]"#,
            ),
            line(
                "2026-10-02T00:01:00.000Z",
                "s1",
                r#""event":"edit","bytes":9,"est_tokens":4,"ids":[],"branch":"feat/x","files":["a.rs"]"#,
            ),
            line(
                "2026-10-02T00:01:00.000Z",
                "s1",
                r#""event":"in-context","in_context":["A"],"files":["a.rs"]"#,
            ),
            line(
                "2026-10-02T00:02:00.000Z",
                "s1",
                r#""event":"call","cmd":"add","outcome":"ok""#,
            ),
            line(
                "2026-10-02T00:03:00.000Z",
                "s1",
                r#""event":"call","cmd":"add","outcome":"reject","reason":"x""#,
            ),
            line(
                "2026-10-02T00:03:30.000Z",
                "s1",
                r#""event":"call","cmd":"add","outcome":"reject","reason":"shape_gate""#,
            ),
            line(
                "2026-10-02T00:04:00.000Z",
                "s1",
                r#""event":"call","cmd":"add","outcome":"ok","who":"human""#,
            ),
            line(
                "2026-10-03T00:00:00.000Z",
                "s2",
                r#""event":"edit","bytes":0,"est_tokens":0,"ids":[]"#,
            ),
        ];
        parse(&lines.join("\n"), Path::new("/state"), &[])
    }

    #[test]
    fn a_session_row_reads_the_aggregates_rules() {
        let run = FlowRun {
            session: Some("/p/s2.jsonl".into()),
            ..Default::default()
        };
        let (list, sp) = sessions(&fixture(), &HashMap::new(), &[run]);
        let s1 = list.iter().find(|v| v.session == "s1").unwrap();
        assert_eq!((s1.shown, s1.scored, s1.in_context, s1.acted), (1, 1, 1, 1));
        assert_eq!((s1.edits, s1.calls, s1.rejects), (1, 2, 1));
        assert_eq!(s1.pushed_tokens["session-start"], 3);
        assert_eq!(s1.branch.as_deref(), Some("feat/x"));
        assert_eq!(s1.first, "2026-10-02T00:00:00.000Z");
        assert_eq!(s1.rows_filed, None); // no log loaded
        // newest first
        assert_eq!(list[0].session, "s2");
        assert!(list[0].in_run && !s1.in_run);
        assert_eq!(
            (sp.pushed.with.sessions, sp.pushed.without.sessions),
            (1, 1)
        );
        assert_eq!(
            (sp.in_run.with.sessions, sp.in_run.without.sessions),
            (1, 1)
        );
        assert_eq!(sp.pushed.with.rejects, 1);
    }

    #[test]
    fn a_unique_prefix_finds_one_and_an_ambiguous_one_names_the_stems() {
        let (list, _) = sessions(&fixture(), &HashMap::new(), &[]);
        assert_eq!(find_session(&list, "s1").unwrap().len(), 1);
        assert_eq!(
            find_session(&list, "/h/x/s2.jsonl").unwrap()[0].session,
            "s2"
        );
        assert!(find_session(&list, "s").unwrap_err().contains("s0, s1, s2"));
        assert!(
            find_session(&list, "zz")
                .unwrap_err()
                .contains("no session")
        );
    }
}
