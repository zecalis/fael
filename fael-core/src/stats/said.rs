//! Yield per line kind (PLAN-fael-say-gate chunk 3): of the lines a hook
//! said (`said` on its usage line), how many the same session acted on
//! after. Earned per kind (SPEC-fael-say-gate):
//!
//! - `row` / `brief` / `note` — the id was in context at an edit
//!   (`in_context`, `in_context_notes`) or retired within a day
//! - `ask` — the id was closed, superseded or bumped within a day
//! - `pointer` / `bodies` — a later pull (`found` line) by that key / an id
//! - `count` — a later pull by the call the line printed: its files, a
//!   directory over one, or its key
//! - `notice` — the session filed a row after it
//!
//! An upper bound: the agent may have done it anyway — use it only to cut.
//! Pure: kept usage rows and loaded logs in.

use super::capture::mine;
use super::parse::Parsed;
use super::retire::{RETIRE_WINDOW_MS, retire_times};
use crate::{Log, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct KindYield {
    /// `said` entries of this kind: one per row id, key or file set.
    pub said: usize,
    /// Of those, acted on later in the same session.
    pub earned: usize,
}

/// A pull's query, from its `found` line.
pub(super) struct Pull<'a> {
    pub(super) ms: i64,
    pub(super) key: Option<&'a str>,
    pub(super) files: Vec<&'a str>,
    /// The id a `find <id>` named.
    pub(super) id: Option<&'a str>,
}

type Session<'a> = (&'a str, &'a str);

pub(super) fn strs<'a>(v: &'a serde_json::Value, k: &str) -> impl Iterator<Item = &'a str> {
    v[k].as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i.as_str())
}

/// A pull that takes the call a count line printed. Its key is
/// `<files>|<what>`: `file` (`fael find --files <files>`), `dir:<dirs>`
/// (`--files src/`), `key:<key>` (`--key <key>`), `keys` (`--key <key>`, up to
/// three named — any key pull counts, an upper bound like the rest). The
/// pull's files are normalised (`src/` reads `src`), so a path covers a file
/// at a `/` boundary: `src` covers `src/a.rs`, `src/a` does not.
pub(super) fn counted(key: &str, p: &Pull) -> bool {
    let (files, what) = key.split_once('|').unwrap_or((key, "file"));
    let covers = |f: &str| {
        let f = f.trim_end_matches('/');
        files
            .split(',')
            .any(|k| k == f || k.strip_prefix(f).is_some_and(|r| r.starts_with('/')))
    };
    match what.strip_prefix("key:") {
        Some(k) => p.key == Some(k),
        None if what == "keys" => p.key.is_some(),
        None => p.files.iter().any(|f| covers(f)),
    }
}

/// Every kind a hook says, zeros included, so a kind never said still shows.
pub const KINDS: [&str; 8] = [
    "row", "note", "brief", "ask", "pointer", "count", "bodies", "notice",
];

pub(super) fn yields(parsed: &Parsed, logs: &HashMap<String, Log>) -> BTreeMap<String, KindYield> {
    let mut in_ctx: HashSet<(Session, &str)> = HashSet::new();
    let mut pulls: HashMap<Session, Vec<Pull>> = HashMap::new();
    for v in &parsed.kept {
        let (Some(repo), Some(s)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        if v["event"] == "in-context" {
            let ids = strs(v, "in_context").chain(strs(v, "in_context_notes"));
            in_ctx.extend(ids.map(|id| ((repo, s), id)));
        } else if v.get("found").is_some()
            && let Some(ms) = v["ts"].as_str().and_then(ts_ms)
        {
            let q = &v["q"];
            pulls.entry((repo, s)).or_default().push(Pull {
                ms,
                key: q["key"].as_str(),
                files: strs(q, "files").collect(),
                id: q["id"].as_str(),
            });
        }
    }
    let gone: HashMap<&str, HashMap<&str, i64>> = logs
        .iter()
        .map(|(repo, log)| (repo.as_str(), retire_times(log)))
        .collect();
    let mut out: BTreeMap<String, KindYield> = KINDS
        .iter()
        .map(|k| (k.to_string(), KindYield::default()))
        .collect();
    for v in &parsed.kept {
        let (Some(repo), Some(ms), Some(said)) = (
            v["repo"].as_str(),
            v["ts"].as_str().and_then(ts_ms),
            v["said"].as_array(),
        ) else {
            continue;
        };
        let s = v["session"].as_str().map(|s| (repo, s));
        let log = logs.get(repo);
        let retired = |id: &str| {
            gone.get(repo)
                .and_then(|t| t.get(id))
                .is_some_and(|g| *g >= ms && *g - ms <= RETIRE_WINDOW_MS)
        };
        let pulled = |hit: &dyn Fn(&Pull) -> bool| {
            s.and_then(|s| pulls.get(&s))
                .is_some_and(|ps| ps.iter().any(|p| p.ms >= ms && hit(p)))
        };
        // a brief names no key: its rows are the line's `ids`
        let entries = said.iter().flat_map(|e| {
            let (kind, key) = (e["kind"].as_str().unwrap_or(""), e["key"].as_str());
            match (kind, key) {
                ("brief", None) => strs(v, "ids").map(|id| ("brief", id)).collect(),
                _ => vec![(kind, key.unwrap_or(""))],
            }
        });
        for (kind, key) in entries {
            let used = |id: &str| s.is_some_and(|s| in_ctx.contains(&(s, id))) || retired(id);
            let (bucket, earned) = match kind {
                "row" => {
                    let note =
                        log.is_some_and(|l| l.rows.iter().any(|r| r.id == key && r.kind == "note"));
                    (if note { "note" } else { "row" }, used(key))
                }
                "brief" => ("brief", used(key)),
                // the generic clause names no row: nothing to join it to
                "ask" if key == "*" => continue,
                "ask" => ("ask", retired(key)),
                "pointer" => ("pointer", pulled(&|p| p.key == Some(key))),
                "count" => ("count", pulled(&|p| counted(key, p))),
                "bodies" => ("bodies", pulled(&|p| p.id.is_some())),
                "notice" => {
                    let filed = |l: &Log| {
                        l.rows.iter().any(|r| {
                            r.session().is_some()
                                && ts_ms(&r.ts).is_some_and(|t| t >= ms)
                                && s.is_some_and(|(_, s)| mine(r, s, None))
                        })
                    };
                    ("notice", log.is_some_and(filed))
                }
                _ => continue,
            };
            let y = out.entry(bucket.to_string()).or_default();
            y.said += 1;
            y.earned += earned as usize;
        }
    }
    out
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    pub(in crate::stats) fn rows(jsonl: &str) -> Vec<crate::Row> {
        let (mut out, mut w) = (vec![], vec![]);
        crate::log::parse(jsonl.as_bytes(), "t.jsonl", &mut out, &mut w);
        out
    }

    #[test]
    fn each_kind_earns_on_its_own_outcome() {
        let row = |id: &str, kind: &str, extra: &str| {
            format!(
                "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"a.rs\"]{extra}}}\n"
            )
        };
        // N is a note; F is filed by session s1 after the notice; A is closed
        let log = Log {
            rows: rows(
                &(row("D", "decision", "")
                    + &row("N", "note", "")
                    + &row("A", "issue", "")
                    + &row("U", "decision", "")
                    + "{\"v\":1,\"id\":\"F\",\"ts\":\"2026-09-26T00:09:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[\"a.rs\"],\"session\":\"s1\"}\n"),
            ),
            closes: rows(
                "{\"v\":1,\"id\":\"C\",\"ts\":\"2026-09-26T00:08:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"A\"}\n",
            ),
            ..Log::default()
        };
        let line = |min: u8, rest: &str| {
            format!(
                "{{\"ts\":\"2026-09-26T00:0{min}:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",{rest}}}\n"
            )
        };
        // a brief names no key: its rows are the line's ids
        let usage = line(
            0,
            r#""event":"session-start","ids":["D","U"],"said":[{"kind":"brief"}]"#,
        ) + &line(
            1,
            r#""event":"edit","ids":["D","N","U"],"said":[{"kind":"row","key":"D"},{"kind":"row","key":"N"},{"kind":"row","key":"U"},{"kind":"bodies"},{"kind":"count","key":"a.rs,b.rs|dir:b/"},{"kind":"count","key":"a.rs,b.rs|key:k:z"},{"kind":"ask","key":"A"},{"kind":"ask","key":"*"},{"kind":"notice"}]"#,
        ) + &line(
            2,
            r#""event":"prompt","ids":[],"said":[{"kind":"pointer","key":"k:x"},{"kind":"pointer","key":"k:y"}]"#,
        ) + &line(
            3,
            r#""event":"in-context","ids":[],"in_context":["D"],"in_context_notes":["N"]"#,
        ) + &line(4, r#""event":"find","found":["D"],"q":{"key":"k:x"}"#)
            + &line(5, r#""event":"find","found":["D"],"q":{"files":["b.rs"]}"#);
        let p = super::super::parse::parse(
            &usage,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let y = yields(&p, &HashMap::from([("/w/r".to_string(), log)]));
        let got = |k: &str| (y[k].said, y[k].earned);
        assert_eq!(got("row"), (2, 1), "D in context, U never");
        assert_eq!(got("note"), (1, 1));
        assert_eq!(got("ask"), (1, 1), "A closed, * not counted");
        assert_eq!(got("pointer"), (2, 1), "k:x pulled, k:y never");
        assert_eq!(
            got("count"),
            (2, 1),
            "the --files line pulled, the --key line never"
        );
        assert_eq!(got("bodies"), (1, 0), "no find by id");
        assert_eq!(got("notice"), (1, 1), "F filed by s1 after it");
        assert_eq!(got("brief"), (2, 1), "D in context, U never");
        // a pull's outcome line is no injection
        assert_eq!(p.n, 3, "session-start + edit + prompt");
    }

    #[test]
    fn a_count_line_earns_on_the_call_it_printed() {
        let pull = |files: &[&'static str], key: Option<&'static str>| Pull {
            ms: 0,
            key,
            files: files.to_vec(),
            id: None,
        };
        let files = |p: &Pull| counted("src/a.rs,src/b.rs|file", p);
        assert!(files(&pull(&["src/b.rs"], None)), "a file it named");
        assert!(!files(&pull(&["lib/"], None)), "another directory");
        assert!(
            !files(&pull(&["src/a"], None)),
            "a prefix that is no directory"
        );
        assert!(!files(&pull(&[], Some("k:a"))), "a key pull");
        let dir = |p: &Pull| counted("src/a.rs|dir:src/", p);
        assert!(dir(&pull(&["src/"], None)), "`+N more in src/`");
        assert!(dir(&pull(&["src"], None)), "the same call, normalised");
        let key = |p: &Pull| counted("src/a.rs|key:auth:session", p);
        assert!(key(&pull(&[], Some("auth:session"))), "`+N more with #key`");
        assert!(!key(&pull(&[], Some("auth:other"))), "a key it never named");
        assert!(!key(&pull(&["src/a.rs"], None)), "a file pull");
        let keys = |p: &Pull| counted("src/a.rs|keys", p);
        assert!(keys(&pull(&[], Some("any:key"))), "`+N more under 3 keys`");
    }
}
