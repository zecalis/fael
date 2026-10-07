//! The context loop's numbers (PLAN-fael-context-loop §3): does the next piece
//! of work miss the same thing less, without more context? Read the three
//! together — fewer rows or tokens with more repeats is worse, not better.
//! Never inferred: a repeat is the agent's own `--supersedes` of a closed
//! issue, fael never decides two bugs are one. Pure: kept usage rows and
//! loaded logs in.

use super::cross::stem;
use super::parse::Parsed;
use crate::{Log, reverted, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ContextLoop {
    /// Issues filed since the repo's first usage that supersede an issue
    /// already closed: the agent confirmed the bug came back (deduped by id).
    pub confirmed_repeats: usize,
    /// Distinct (session, closed issue) pairs: an edit usage line of the
    /// session named one of the issue's files after its close — the base the
    /// repeats read against. A lower bound (an edit where fael said nothing
    /// and found nothing in context writes no line), so the rate is an upper one.
    pub edits_after_close: usize,
    /// (repo, session, row) units fael said that were then cited or acted on
    /// (`outcomes`, union: cited and acted counts once) — read against the
    /// top-level `est_tokens`.
    pub useful_shows: usize,
}

/// Edit usage lines, as `overlap.rs` reads them.
const EDIT_EVENTS: [&str; 3] = ["edit", "shell-edit", "in-context"];

/// File → the closed issues on it, with their close time.
type OnFile<'a> = HashMap<&'a str, Vec<(&'a str, i64)>>;

/// Each closed issue's close time: a close row, or the close `fael compact`
/// folded into the row. The earliest wins.
fn closed_issues(log: &Log) -> HashMap<&str, i64> {
    let issues: HashSet<&str> = log
        .rows
        .iter()
        .filter(|r| r.kind == "issue")
        .map(|r| r.id.as_str())
        .collect();
    let folded = log
        .rows
        .iter()
        .filter(|r| r.kind == "issue")
        .filter_map(|r| {
            let ts = r.extra.get("closed")?["ts"].as_str()?;
            Some((r.id.as_str(), ts_ms(ts)?))
        });
    let mut out: HashMap<&str, i64> = HashMap::new();
    let closes = log.closes.iter().filter_map(|c| {
        let id = c.reference.as_deref()?;
        issues.contains(id).then_some((id, ts_ms(&c.ts)?))
    });
    for (id, ms) in closes.chain(folded) {
        out.entry(id)
            .and_modify(|m| *m = (*m).min(ms))
            .or_insert(ms);
    }
    out
}

pub(super) fn context_loop(parsed: &Parsed, logs: &HashMap<String, Log>) -> ContextLoop {
    let closed: HashMap<&str, HashMap<&str, i64>> = logs
        .iter()
        .map(|(repo, log)| (repo.as_str(), closed_issues(log)))
        .collect();
    // repos in one clone share the journal: dedup the repeat by its id
    let mut repeats: HashSet<&str> = HashSet::new();
    for (repo, first) in &parsed.first_seen {
        let (Some(log), Some(gone)) = (logs.get(repo), closed.get(repo.as_str())) else {
            continue;
        };
        let rev = reverted(log);
        repeats.extend(
            log.rows
                .iter()
                .filter(|r| r.kind == "issue" && !rev.contains(r.id.as_str()))
                .filter_map(|r| {
                    let ms = ts_ms(&r.ts).filter(|ms| ms >= first)?;
                    let was = gone.get(r.supersedes.as_deref()?)?;
                    (*was <= ms).then_some(r.id.as_str())
                }),
        );
    }
    let mut on_file: HashMap<&str, OnFile> = HashMap::new();
    for (repo, log) in logs {
        let gone = &closed[repo.as_str()];
        let files = on_file.entry(repo.as_str()).or_default();
        for r in log.rows.iter().filter(|r| gone.contains_key(r.id.as_str())) {
            for f in &r.files {
                files
                    .entry(f)
                    .or_default()
                    .push((&r.id, gone[r.id.as_str()]));
            }
        }
    }
    let mut edits: HashSet<(&str, &str)> = HashSet::new();
    for v in &parsed.kept {
        if !v["event"]
            .as_str()
            .is_some_and(|e| EDIT_EVENTS.contains(&e))
        {
            continue;
        }
        let (Some(files), Some(session), Some(ms)) = (
            v["repo"].as_str().and_then(|r| on_file.get(r)),
            v["session"].as_str(),
            v["ts"].as_str().and_then(ts_ms),
        ) else {
            continue;
        };
        let named = super::said::strs(v, "files").filter_map(|f| files.get(f));
        for (id, _) in named.flatten().filter(|(_, at)| *at < ms) {
            edits.insert((stem(session), id));
        }
    }
    ContextLoop {
        confirmed_repeats: repeats.len(),
        edits_after_close: edits.len(),
        useful_shows: super::outcomes::observations(parsed, logs)
            .iter()
            .filter(|o| o.cited || o.acted)
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_repeat_is_a_supersede_of_a_closed_issue_read_against_later_edits() {
        let row = |id: &str, ts: &str, kind: &str, rest: &str| {
            format!(
                "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-0{ts}T00:00:00Z\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"a.rs\"]{rest}}}\n"
            )
        };
        // A closed day 2, B re-files it day 4: a repeat. D supersedes an open
        // issue C (a rewrite, no repeat); E supersedes the closed decision F
        let log = Log {
            rows: super::super::said::tests::rows(
                &(row("A", "1", "issue", "")
                    + &row("B", "4", "issue", r#","supersedes":"A""#)
                    + &row("C", "1", "issue", "")
                    + &row("D", "4", "issue", r#","supersedes":"C""#)
                    + &row("F", "1", "decision", "")
                    + &row("E", "4", "issue", r#","supersedes":"F""#)),
            ),
            closes: super::super::said::tests::rows(
                &(row("X", "2", "close", r#","ref":"A""#)
                    + &row("Y", "2", "close", r#","ref":"F""#)),
            ),
            ..Log::default()
        };
        let edit = |day: u8, s: &str, file: &str| {
            format!(
                "{{\"ts\":\"2026-10-0{day}T00:00:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/{s}.jsonl\",\"event\":\"edit\",\"ids\":[],\"files\":[\"{file}\"]}}\n"
            )
        };
        // s1 is told A (closed within the day: useful) and edits before the
        // close (no count), s2 edits twice after (one pair), s3 another file
        let told = "{\"ts\":\"2026-10-01T00:00:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",\"event\":\"read\",\"ids\":[\"A\"]}\n";
        let usage = told.to_string()
            + &edit(1, "s1", "a.rs")
            + &edit(3, "s2", "a.rs")
            + &edit(3, "s2", "a.rs")
            + &edit(3, "s3", "b.rs");
        let p = super::super::parse::parse(
            &usage,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let c = context_loop(&p, &HashMap::from([("/w/r".to_string(), log)]));
        assert_eq!(
            c,
            ContextLoop {
                confirmed_repeats: 1,
                edits_after_close: 1,
                useful_shows: 1,
            }
        );
    }

    #[test]
    fn a_folded_close_counts_a_restored_repeat_does_not_and_a_cited_acted_show_is_one() {
        let row = |id: &str, ts: &str, file: &str, rest: &str| {
            format!(
                "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-0{ts}T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[\"{file}\"]{rest}}}\n"
            )
        };
        // G's close `fael compact` folded into the row, H re-files it: a repeat.
        // L re-files the closed K, then a restore reverts that edge: none.
        let folded =
            r#","closed":{"id":"t-1","ts":"2026-10-02T00:00:00Z","by":"w","text":"fixed"}"#;
        let log = Log {
            rows: super::super::said::tests::rows(
                &(row("G", "1", "g.rs", folded)
                    + &row("H", "4", "g.rs", r#","supersedes":"G""#)
                    + &row("K", "1", "k.rs", "")
                    + &row("L", "4", "k.rs", r#","supersedes":"K""#)
                    + "{\"v\":1,\"id\":\"R\",\"ts\":\"2026-10-05T00:00:00Z\",\"by\":\"w\",\"text\":\"t\",\"restores\":\"L\"}\n"
                    + &row("M", "1", "m.rs", "")
                    + &row("N", "1", "n.rs", "")),
            ),
            closes: super::super::said::tests::rows(concat!(
                "{\"v\":1,\"id\":\"X\",\"ts\":\"2026-10-02T00:00:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"K\"}\n",
                "{\"v\":1,\"id\":\"Y\",\"ts\":\"2026-10-01T12:00:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"M\"}\n",
            )),
            ..Log::default()
        };
        let line = |at: &str, rest: &str| {
            format!(
                "{{\"ts\":\"2026-10-0{at}Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",{rest}}}\n"
            )
        };
        // M is said, cited and closed within the day: one useful show, not
        // two. N is said and only cited: one more. s1 edits g.rs after G's
        // folded close: one pair.
        let usage = line("1T00:00:00.000", r#""event":"read","ids":["M","N"]"#)
            + &line(
                "1T00:01:00.000",
                r#""event":"outcome","ids":[],"cited":["M","N"]"#,
            )
            + &line(
                "3T00:00:00.000",
                r#""event":"edit","ids":[],"files":["g.rs"]"#,
            );
        let p = super::super::parse::parse(
            &usage,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let c = context_loop(&p, &HashMap::from([("/w/r".to_string(), log)]));
        assert_eq!(
            c,
            ContextLoop {
                confirmed_repeats: 1,
                edits_after_close: 1,
                useful_shows: 2,
            }
        );
    }
}
